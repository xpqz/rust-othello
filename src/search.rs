//! Fixed-depth negamax with alpha-beta and the search result contract.

use std::time::{Duration, Instant};

use crate::board::{Board, empty_count, final_score, flips, legal_moves, pass, play, squares};
use crate::score::{EVAL_BOUND, INF, terminal_score};
use crate::tt::{Bound, Domain, Entry, NO_MOVE, Table, zobrist};

#[derive(Clone, Copy, Debug)]
pub struct SearchConfig {
    pub eval: fn(Board) -> i32,
    pub endgame_threshold: u32,
    pub ordering: bool,
    pub probcut: Option<ProbCut>,
    /// Read only by `book::play`; `search_tt` ignores it.
    pub book: bool,
}

impl SearchConfig {
    pub fn standard() -> Self {
        Self {
            eval: crate::pattern::evaluate,
            endgame_threshold: 18,
            ordering: true,
            probcut: Some(ProbCut::FITTED),
            book: false,
        }
    }
}

/// Selective cuts (docs/design/probcut2.md): a shallow null-window search
/// predicts the deep one, `v_d = a·v_s + b` with error σ. A table serves one
/// policy: callers clear it when any of these fields change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbCut {
    /// Margin in hundredths of σ; at most 400 is used.
    pub t: u32,
    /// σ = s0 + s1·empties + s2·depth, in hundredths of a centidisc.
    pub s0: i32,
    pub s1: i32,
    pub s2: i32,
    /// Slope in thousandths (500..=1500 used) and intercept in centidiscs.
    pub a_milli: i32,
    pub b: i32,
    /// 0: shallow depth 2·⌊d/4⌋ + d mod 2; k > 0: d − k.
    pub shallow: u32,
    pub d_min: u32,
    pub d_max: u32,
    pub e_min: u32,
    pub e_max: u32,
}

fn ceil_div(n: i64, k: i64) -> i64 {
    n.div_euclid(k) + i64::from(n.rem_euclid(k) != 0)
}

impl ProbCut {
    /// The audited calibration fit (`docs/tuning/probcut2_audit.log`, commit
    /// 2c9ab08); `t` picked by the registered tuning matches
    /// (`docs/tuning/probcut2_tune_80.log`, `probcut2_tune_120.log`).
    pub const FITTED: ProbCut = ProbCut {
        t: 80,
        s0: 90066,
        s1: -514,
        s2: -1180,
        a_milli: 973,
        b: 30,
        shallow: 0,
        d_min: 4,
        d_max: 14,
        e_min: 21,
        e_max: 56,
    };

    /// σ in centidiscs, rounded up, within `1..=2·EVAL_BOUND`.
    pub fn sigma(&self, d: u32, e: u32) -> i64 {
        let (d, e) = (i64::from(d.min(64)), i64::from(e.min(64)));
        let raw = i64::from(self.s0) + i64::from(self.s1) * e + i64::from(self.s2) * d;
        ceil_div(raw, 100).clamp(1, 2 * i64::from(EVAL_BOUND))
    }
}

/// The shallow depth for remaining depth `d`, if `1 <= s < d`.
pub fn shallow_depth(p: &ProbCut, d: u32) -> Option<u32> {
    match p.shallow {
        0 => (d >= 3).then(|| 2 * (d / 4) + (d & 1)),
        gap => d.checked_sub(gap).filter(|&s| s >= 1),
    }
}

/// The values the shallow search must reach: at least `H` to cut high, at
/// most `L` to cut low, each rounded toward the harder side.
pub fn probcut_bounds(p: &ProbCut, d: u32, e: u32, alpha: i32, beta: i32) -> (i64, i64) {
    let margin = ceil_div(i64::from(p.t.min(400)) * p.sigma(d, e), 100);
    let a = i64::from(p.a_milli.clamp(500, 1500));
    let b = i64::from(p.b.clamp(-EVAL_BOUND, EVAL_BOUND));
    let high = ceil_div((i64::from(beta) + margin - b) * 1000, a);
    let low = ((i64::from(alpha) - margin - b) * 1000).div_euclid(a);
    (high, low)
}

/// The ProbCut check at a heuristic node: `Some(Some(bound))` on a cut (`beta`
/// high, `alpha` low), `Some(None)` without one, `None` when stopped.
pub fn probcut(board: Board, d: u32, alpha: i32, beta: i32, ctx: &mut Ctx) -> Option<Option<i32>> {
    let Some(p) = ctx.config.probcut else {
        return Some(None);
    };
    let e = empty_count(board);
    let shallow = shallow_depth(&p, d)
        .filter(|_| (p.d_min..=p.d_max).contains(&d) && (p.e_min..=p.e_max).contains(&e));
    let Some(s) = shallow else {
        return Some(None);
    };
    ctx.probcut_tries += 1;
    // Every search value lies within ±EVAL_BOUND, so a threshold beyond the
    // far bound decides nothing and one beyond the near bound decides as the
    // bound itself does.
    let bound = i64::from(EVAL_BOUND);
    let (high, low) = probcut_bounds(&p, d, e, alpha, beta);
    if high <= bound {
        let high = high.max(-bound) as i32;
        if negamax(board, s, high - 1, high, ctx)? >= high {
            ctx.probcut_cuts[0] += 1;
            return Some(Some(beta));
        }
    }
    if low >= -bound {
        let low = low.min(bound) as i32;
        if negamax(board, s, low, low + 1, ctx)? <= low {
            ctx.probcut_cuts[1] += 1;
            return Some(Some(alpha));
        }
    }
    Some(None)
}

pub const NO_DEADLINE: u64 = u64::MAX;
/// An exact solve may run to this many times the move time.
const SOLVE_ALLOWANCE: u64 = 4;
/// Depth of the heuristic search that stands in for a solve that ran out of time.
const FALLBACK_DEPTH: u32 = 8;
const EXACT_ORDER_DEPTH: u32 = 2;
/// Exact search with fewer empties orders moves without counting replies.
const PLAIN_ORDER_EMPTIES: u32 = 7;
const EVAL_ORDER_DEPTH: u32 = 4;
const REPLY_WEIGHT: i32 = 300;
const HINT_BONUS: i32 = 800;

#[derive(Clone, Copy, Debug)]
pub struct SearchLimits {
    pub max_depth: u32,
    /// Move time from entry; an exact solve may use `SOLVE_ALLOWANCE` times it.
    pub time_ms: u64,
    pub max_nodes: u64,
    /// No iteration starts after this many ms; the exact solve ignores it.
    pub start_ms: u64,
    /// Early exits for a settled heuristic search; the exact solve ignores them.
    pub early: Early,
}

/// A heuristic search stops starting iterations once it is settled
/// (`docs/design/time.md` amendment 1): the best move unchanged over the
/// last `stable` iterations, or |score| at least `decided`, and at least
/// `share`% (at most 100) of `start_ms` gone. Zero turns a rule off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Early {
    pub stable: u8,
    pub decided: u16,
    pub share: u8,
}

impl Early {
    pub const OFF: Early = Early {
        stable: 0,
        decided: 0,
        share: 0,
    };
}

/// Whether a settled search stops after its completed iterations
/// `history`, as `(best, score)` in order, `elapsed` after entry.
pub fn stops(_elapsed: Duration, _start_ms: u64, _early: &Early, _history: &[(u8, i32)]) -> bool {
    false
}

/// A clock allocation policy; only the registered `CLOCK_POLICIES` exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockPolicy {
    weight: (u64, u64),
    reserve: u64,
    cap: u64,
    early: Early,
    forced: bool,
}

/// The four registered variants (`docs/design/time.md` amendment 1).
pub const CLOCK_POLICIES: [ClockPolicy; 4] = {
    let base = ClockPolicy {
        weight: (3, 2),
        reserve: 4,
        cap: 2,
        early: Early::OFF,
        forced: false,
    };
    [base, base, base, base]
};

impl ClockPolicy {
    pub fn variant(_k: usize) -> Option<&'static ClockPolicy> {
        None
    }

    /// Whether a clocked agent plays a lone legal move without searching.
    pub fn forced(&self) -> bool {
        self.forced
    }
}

impl SearchLimits {
    pub fn new(max_depth: u32, time_ms: u64, max_nodes: u64) -> Self {
        Self {
            max_depth,
            time_ms,
            max_nodes,
            start_ms: time_ms,
            early: Early::OFF,
        }
    }

    pub fn depth(max_depth: u32) -> Self {
        Self {
            max_depth,
            time_ms: NO_DEADLINE,
            max_nodes: 0,
            start_ms: NO_DEADLINE,
            early: Early::OFF,
        }
    }
}

/// Clocks are clamped to this, so allocation cannot overflow.
pub const MAX_CLOCK_MS: u64 = 1 << 40;

/// Limits for one move from `remaining_ms` on the game clock
/// (`docs/design/time.md` § Allocation).
pub fn allot(board: Board, remaining_ms: u64, threshold: u32) -> SearchLimits {
    let r = remaining_ms.min(MAX_CLOCK_MS);
    let empties = empty_count(board);
    // Target `t` and in-search cap; the exact solve takes 4 × its cap.
    let (t, cap) = if empties <= threshold {
        (r / 8, r / 8)
    } else {
        let moves = u64::from((empties - threshold).div_ceil(2)) + 4;
        let t = if empties <= 44 {
            3 * r / (2 * moves)
        } else {
            r / moves
        };
        (t, (2 * t).min(r / 4))
    };
    let time_ms = cap.max(1);
    SearchLimits {
        max_depth: 64,
        time_ms,
        max_nodes: 0,
        start_ms: t.min(time_ms).max(1),
        early: Early::OFF,
    }
}

/// `allot` under a registered policy.
pub fn allot_with(
    board: Board,
    remaining_ms: u64,
    threshold: u32,
    _policy: &ClockPolicy,
) -> SearchLimits {
    allot(board, remaining_ms, threshold)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchStatus {
    Complete,
    Partial,
    Fallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub best: u8,
    pub score: i32,
    /// 0 for an exact solve; heuristic iteration depth otherwise.
    pub depth: u32,
    pub nodes: u64,
    pub status: SearchStatus,
    pub solved: bool,
}

impl SearchResult {
    fn new(best: u8, score: i32, depth: u32, nodes: u64, status: SearchStatus) -> Self {
        Self {
            best,
            score,
            depth,
            nodes,
            status,
            solved: false,
        }
    }
}

pub struct Ctx<'a> {
    pub config: &'a SearchConfig,
    pub nodes: u64,
    pub probcut_tries: u64,
    /// High and low cuts.
    pub probcut_cuts: [u64; 2],
    max_nodes: u64,
    deadline: Option<Instant>,
    table: Option<&'a mut Table>,
    exact: bool,
}

impl<'a> Ctx<'a> {
    pub fn new(config: &'a SearchConfig) -> Self {
        Self {
            config,
            nodes: 0,
            probcut_tries: 0,
            probcut_cuts: [0; 2],
            max_nodes: 0,
            deadline: None,
            table: None,
            exact: false,
        }
    }

    pub fn set_table(&mut self, table: &'a mut Table) {
        self.table = Some(table);
    }

    pub fn set_node_limit(&mut self, max_nodes: u64) {
        self.max_nodes = max_nodes;
    }

    fn expired(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    fn stop(&self) -> bool {
        (self.max_nodes > 0 && self.nodes > self.max_nodes)
            || (self.nodes & 1023 == 0 && self.expired())
    }

    /// Before an iteration: the node limit, or the start cutoff `admit`.
    fn stop_iteration(&self, admit: Option<Instant>) -> bool {
        (self.max_nodes > 0 && self.nodes > self.max_nodes)
            || admit.is_some_and(|admit| Instant::now() >= admit)
    }
}

const CORNERS: u64 = 0x8100_0000_0000_0081;
const XC: u64 = 0x42c3_0000_0000_c342;

/// Without an evaluation: preferred move, corners, others, X and C squares,
/// then fewest opponent replies from depth 2. With one: the child's value to
/// the opponent plus a charge per reply, the preferred move at a discount.
/// `preferred` must be a square (below 64).
pub fn ordered_moves(
    board: Board,
    moves: u64,
    depth: u32,
    preferred: Option<u8>,
    eval: Option<fn(Board) -> i32>,
) -> ([u8; 64], usize) {
    let mut order = [0u8; 64];
    let mut keys = [0u32; 64];
    let mut len = 0;
    if eval.is_none() && depth < 2 {
        let hint = preferred.map_or(0, |square| 1 << square) & moves;
        let rest = moves & !hint;
        for class in [hint, rest & CORNERS, rest & !(CORNERS | XC), rest & XC] {
            for square in squares(class) {
                order[len] = square;
                len += 1;
            }
        }
        return (order, len);
    }
    let replies = |child: Board| legal_moves(child.me, child.opp).count_ones();
    for square in squares(moves) {
        let hinted = Some(square) == preferred;
        let rank = match (hinted, CORNERS >> square & 1, XC >> square & 1) {
            (true, ..) => 0,
            (false, 1, _) => 1,
            (false, 0, 1) => 3,
            _ => 2,
        };
        let key = match eval {
            Some(eval) => {
                let child = play(board, square);
                let value = eval(child) + REPLY_WEIGHT * replies(child) as i32;
                ((value + EVAL_BOUND + HINT_BONUS * i32::from(!hinted)) as u32) << 6
            }
            None => (rank << 16) | (replies(play(board, square)) << 8),
        };
        keys[len] = key | square as u32;
        order[len] = square;
        len += 1;
    }
    for index in 0..len {
        let best = (index..len).min_by_key(|&probe| keys[probe]).unwrap();
        keys.swap(index, best);
        order.swap(index, best);
    }
    (order, len)
}

fn natural(moves: u64) -> ([u8; 64], usize) {
    let mut order = [0u8; 64];
    let mut len = 0;
    for square in squares(moves) {
        order[len] = square;
        len += 1;
    }
    (order, len)
}

/// The final score for the mover of a board with one empty square: the mover
/// plays it if that flips a disc, otherwise the opponent does, otherwise the
/// game is already over.
fn last_square(board: Board) -> i32 {
    let square = (!(board.me | board.opp)).trailing_zeros() as u8;
    let filled = |board: Board, flipped: u64| {
        final_score(Board {
            me: board.me | 1 << square | flipped,
            opp: board.opp & !flipped,
        })
    };
    match (flips(board, square), flips(pass(board), square)) {
        (0, 0) => final_score(board),
        (0, theirs) => -filled(pass(board), theirs),
        (mine, _) => filled(board, mine),
    }
}

/// The final score for the mover of a board with two empty squares, or a
/// fail-soft bound on it: each square the mover can play is tried and the
/// other settled by `last_square`; if the mover can play neither, the same
/// for the opponent. The window is in centidiscs.
fn last_two(board: Board, alpha: i32, beta: i32) -> i32 {
    let best = |board: Board, beta: i32| {
        let mut best = None;
        for square in squares(!(board.me | board.opp)) {
            let flipped = flips(board, square);
            if flipped != 0 {
                let child = Board {
                    me: board.opp & !flipped,
                    opp: board.me | 1 << square | flipped,
                };
                let value = -last_square(child);
                best = best.max(Some(value));
                if terminal_score(value) >= beta {
                    break;
                }
            }
        }
        best
    };
    match best(board, beta) {
        Some(value) => value,
        None => best(pass(board), -alpha).map_or(final_score(board), |value| -value),
    }
}

pub fn negamax(board: Board, depth: u32, mut alpha: i32, beta: i32, ctx: &mut Ctx) -> Option<i32> {
    ctx.nodes += 1;
    if ctx.stop() {
        return None;
    }
    let exact = ctx.exact;
    match (exact, empty_count(board)) {
        (true, 1) => return Some(terminal_score(last_square(board))),
        (true, 2) => return Some(terminal_score(last_two(board, alpha, beta))),
        _ => {}
    }
    let moves = legal_moves(board.me, board.opp);
    if moves == 0 {
        if legal_moves(board.opp, board.me) == 0 {
            return Some(terminal_score(final_score(board)));
        }
        return negamax(pass(board), depth, -beta, -alpha, ctx).map(|value| -value);
    }
    if !exact && depth == 0 {
        return Some((ctx.config.eval)(board));
    }
    let key = ctx.table.as_ref().map(|_| zobrist(board));
    let entry = key.and_then(|key| ctx.table.as_ref()?.probe(key)).copied();
    let cutoff = entry.and_then(|entry| {
        if exact {
            entry.endgame_cutoff(alpha, beta)
        } else {
            entry.cutoff(depth, alpha, beta)
        }
    });
    if cutoff.is_some() {
        return cutoff;
    }
    if !exact && let Some(cut) = probcut(board, depth, alpha, beta, ctx)? {
        return Some(cut);
    }
    let alpha_orig = alpha;
    let hint = (!exact)
        .then(|| entry.and_then(|entry| entry.move_hint()))
        .flatten();
    let mut best = -INF;
    let mut chosen = NO_MOVE;
    // Ordering by reply count alone is for exact search with enough empties.
    // Midgame nodes use the class order, or from its depth up the evaluation
    // order, which weighs replies itself.
    let count_replies = exact && empty_count(board) >= PLAIN_ORDER_EMPTIES;
    let order_depth = if count_replies { EXACT_ORDER_DEPTH } else { 0 };
    let (order, len) = if ctx.config.ordering {
        let eval = (!exact && depth >= EVAL_ORDER_DEPTH).then_some(ctx.config.eval);
        ordered_moves(board, moves, order_depth, hint, eval)
    } else {
        natural(moves)
    };
    let next = if exact { depth } else { depth - 1 };
    for &square in &order[..len] {
        let value = scout(play(board, square), next, alpha, beta, best == -INF, ctx)?;
        if value > best {
            best = value;
            chosen = square;
        }
        alpha = alpha.max(value);
        if alpha >= beta {
            break;
        }
    }
    if let (Some(table), Some(key)) = (ctx.table.as_mut(), key) {
        let flag = match (best <= alpha_orig, best >= beta) {
            (true, _) => Bound::Upper,
            (_, true) => Bound::Lower,
            _ => Bound::Exact,
        };
        let entry = if exact {
            Entry {
                key,
                value: best,
                depth: 0,
                flag,
                domain: Domain::Endgame,
                mv: chosen,
            }
        } else {
            Entry::heuristic(key, best, depth as u8, flag, chosen)
        };
        table.store(entry);
    }
    Some(best)
}

/// The mover's value of `child`. After the first move a null window is tried,
/// and only a value strictly inside the window is searched again in full.
fn scout(
    child: Board,
    depth: u32,
    alpha: i32,
    beta: i32,
    first: bool,
    ctx: &mut Ctx,
) -> Option<i32> {
    if !first && beta - alpha > 1 {
        let value = -negamax(child, depth, -alpha - 1, -alpha, ctx)?;
        if value <= alpha || value >= beta {
            return Some(value);
        }
    }
    negamax(child, depth, -beta, -alpha, ctx).map(|value| -value)
}

pub fn solve(board: Board, alpha: i32, beta: i32, ctx: &mut Ctx) -> Option<i32> {
    ctx.exact = true;
    let value = negamax(board, EXACT_ORDER_DEPTH, alpha, beta, ctx);
    ctx.exact = false;
    value
}

pub fn solve_wdl(board: Board, ctx: &mut Ctx) -> Option<i32> {
    let moves = legal_moves(board.me, board.opp);
    assert!(moves != 0, "solve_wdl requires a legal root move");
    let best = moves.trailing_zeros() as u8;
    wdl_root(board, moves, best, ctx).map(|(_, wdl)| wdl)
}

fn wdl_root(board: Board, moves: u64, best: u8, ctx: &mut Ctx) -> Option<(u8, i32)> {
    ctx.exact = true;
    let found = root(board, moves, EXACT_ORDER_DEPTH, best, -1, 1, ctx);
    ctx.exact = false;
    let (square, value) = found?;
    let wdl = value.signum();
    let flag = match wdl {
        1 => Bound::Lower,
        -1 => Bound::Upper,
        _ => Bound::Exact,
    };
    if let Some(table) = ctx.table.as_mut() {
        table.store(Entry {
            key: zobrist(board),
            value: wdl,
            depth: 0,
            flag,
            domain: Domain::Endgame,
            mv: square,
        });
    }
    Some((square, wdl))
}

pub fn search(board: Board, depth: u32, config: &SearchConfig) -> SearchResult {
    let (moves, first, fallback) = root_setup(board, config);
    if depth == 0 {
        return fallback;
    }
    if empty_count(board) <= config.endgame_threshold {
        return search_inner(board, SearchLimits::depth(depth), config, None);
    }
    let mut ctx = Ctx::new(config);
    let (best, score) = root(board, moves, depth, first, -INF, INF, &mut ctx).unwrap();
    SearchResult::new(best, score, depth, ctx.nodes, SearchStatus::Complete)
}

/// Deepens to `max_depth`; on tied root scores it keeps the previous best,
/// unlike `search`, which keeps the lowest square.
pub fn search_iterative(board: Board, max_depth: u32, config: &SearchConfig) -> SearchResult {
    search_limited(board, SearchLimits::depth(max_depth), config)
}

pub fn search_limited(board: Board, limits: SearchLimits, config: &SearchConfig) -> SearchResult {
    search_inner(board, limits, config, None)
}

pub fn search_tt(
    board: Board,
    limits: SearchLimits,
    config: &SearchConfig,
    table: &mut Table,
) -> SearchResult {
    search_inner(board, limits, config, Some(table))
}

fn search_inner(
    board: Board,
    limits: SearchLimits,
    config: &SearchConfig,
    mut table: Option<&mut Table>,
) -> SearchResult {
    let start = Instant::now();
    let deadline = start.checked_add(Duration::from_millis(limits.time_ms));
    let admit = start.checked_add(Duration::from_millis(limits.start_ms));
    let (moves, fallback, fallback_result) = root_setup(board, config);
    if empty_count(board) <= config.endgame_threshold {
        let allowance = limits.time_ms.saturating_mul(SOLVE_ALLOWANCE);
        let mut ctx = Ctx {
            max_nodes: limits.max_nodes,
            deadline: start.checked_add(Duration::from_millis(allowance)),
            exact: true,
            ..Ctx::new(config)
        };
        if let Some(table) = table.as_deref_mut() {
            ctx.set_table(table);
        }
        let Some((wdl_best, wdl)) = wdl_root(board, moves, fallback, &mut ctx) else {
            let used = ctx.nodes;
            return fallback_search(board, limits, config, table, used, fallback_result);
        };
        ctx.exact = true;
        let (alpha, beta) = match wdl {
            1 => (0, INF),
            -1 => (-INF, 0),
            _ => (-1, 1),
        };
        let Some((square, score)) = root(
            board,
            moves,
            EXACT_ORDER_DEPTH,
            fallback,
            alpha,
            beta,
            &mut ctx,
        ) else {
            return SearchResult::new(wdl_best, wdl, 0, ctx.nodes, SearchStatus::Partial);
        };
        let mut result = SearchResult::new(square, score, 0, ctx.nodes, SearchStatus::Complete);
        result.solved = true;
        return result;
    }
    let mut best = fallback;
    let mut nodes = 0;
    let mut result = fallback_result;
    for depth in 1..=limits.max_depth {
        let mut ctx = Ctx {
            nodes,
            max_nodes: limits.max_nodes,
            deadline,
            table: table.as_deref_mut(),
            ..Ctx::new(config)
        };
        if ctx.stop_iteration(admit) {
            break;
        }
        let Some((square, score)) = root(board, moves, depth, best, -INF, INF, &mut ctx) else {
            break;
        };
        best = square;
        nodes = ctx.nodes;
        result = SearchResult::new(square, score, depth, nodes, SearchStatus::Complete);
    }
    if result.depth > 0 && result.depth < limits.max_depth {
        result.status = SearchStatus::Partial;
    }
    result
}

/// After a win/draw/loss pass that did not finish: a heuristic search to
/// `FALLBACK_DEPTH` on whatever node budget is left, so that the move is not
/// the arbitrary lowest square. A used-up budget gets the static fallback.
fn fallback_search(
    board: Board,
    limits: SearchLimits,
    config: &SearchConfig,
    table: Option<&mut Table>,
    used: u64,
    fallback: SearchResult,
) -> SearchResult {
    let spare = match limits.max_nodes {
        0 => 0,
        max if max > used => max - used,
        _ => return fallback,
    };
    let heuristic = SearchConfig {
        endgame_threshold: 0,
        ..*config
    };
    let limits = SearchLimits::new(FALLBACK_DEPTH, NO_DEADLINE, spare);
    let mut result = search_inner(board, limits, &heuristic, table);
    result.nodes += used;
    if result.depth > 0 {
        result.status = SearchStatus::Partial;
    }
    result
}

fn root_setup(board: Board, config: &SearchConfig) -> (u64, u8, SearchResult) {
    let moves = legal_moves(board.me, board.opp);
    assert!(moves != 0, "search requires a legal root move");
    let first = moves.trailing_zeros() as u8;
    let fallback = SearchResult::new(first, (config.eval)(board), 0, 0, SearchStatus::Fallback);
    (moves, first, fallback)
}

fn root(
    board: Board,
    moves: u64,
    depth: u32,
    best: u8,
    mut alpha: i32,
    beta: i32,
    ctx: &mut Ctx,
) -> Option<(u8, i32)> {
    let mut chosen = best;
    let mut score = -INF;
    let (order, len) = if ctx.config.ordering {
        ordered_moves(board, moves, depth, Some(best), None)
    } else {
        natural(moves)
    };
    let next = if ctx.exact { depth } else { depth - 1 };
    for &square in &order[..len] {
        let value = scout(play(board, square), next, alpha, beta, score == -INF, ctx)?;
        if value > score {
            score = value;
            chosen = square;
        }
        alpha = alpha.max(value);
        if alpha >= beta {
            break;
        }
    }
    if ctx.exact
        && let Some(table) = ctx.table.as_mut()
    {
        table.store(Entry {
            key: zobrist(board),
            value: score,
            depth: 0,
            flag: Bound::Exact,
            domain: Domain::Endgame,
            mv: chosen,
        });
    }
    Some((chosen, score))
}
