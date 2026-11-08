//! Fixed-depth negamax with alpha-beta and the search result contract.

use std::time::{Duration, Instant};

use crate::board::{Board, final_score, legal_moves, pass, play, squares};
use crate::score::{INF, terminal_score};

#[derive(Clone, Copy, Debug)]
pub struct SearchConfig {
    pub eval: fn(Board) -> i32,
    pub endgame_threshold: u32,
}

pub const NO_DEADLINE: u64 = u64::MAX;

#[derive(Clone, Copy, Debug)]
pub struct SearchLimits {
    pub max_depth: u32,
    pub time_ms: u64,
    pub max_nodes: u64,
}

impl SearchLimits {
    pub fn new(max_depth: u32, time_ms: u64, max_nodes: u64) -> Self {
        Self {
            max_depth,
            time_ms,
            max_nodes,
        }
    }

    pub fn depth(max_depth: u32) -> Self {
        Self {
            max_depth,
            time_ms: NO_DEADLINE,
            max_nodes: 0,
        }
    }
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
    max_nodes: u64,
    deadline: Option<Instant>,
}

impl<'a> Ctx<'a> {
    pub fn new(config: &'a SearchConfig) -> Self {
        Self {
            config,
            nodes: 0,
            max_nodes: 0,
            deadline: None,
        }
    }

    fn expired(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    fn stop(&self) -> bool {
        (self.max_nodes > 0 && self.nodes > self.max_nodes)
            || (self.nodes & 1023 == 0 && self.expired())
    }

    fn stop_iteration(&self) -> bool {
        (self.max_nodes > 0 && self.nodes > self.max_nodes) || self.expired()
    }
}

const CORNERS: u64 = 0x8100_0000_0000_0081;
const XC: u64 = 0x42c3_0000_0000_c342;

pub fn ordered_moves(b: Board, m: u64, d: u32, p: Option<u8>) -> ([u8; 64], usize) {
    let mut order = [0u8; 64];
    let mut keys = [0u32; 64];
    let mut len = 0;
    for square in squares(m) {
        let rank = match (
            Some(square) == p,
            CORNERS & (1 << square) != 0,
            XC & (1 << square) != 0,
        ) {
            (true, ..) => 0,
            (false, true, _) => 1,
            (false, false, true) => 3,
            _ => 2,
        };
        let count = if d >= 2 {
            let child = play(b, square);
            legal_moves(child.me, child.opp).count_ones()
        } else {
            0
        };
        keys[len] = (rank << 16) | (count << 8) | square as u32;
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

pub fn negamax(board: Board, depth: u32, mut alpha: i32, beta: i32, ctx: &mut Ctx) -> Option<i32> {
    ctx.nodes += 1;
    if ctx.stop() {
        return None;
    }
    let moves = legal_moves(board.me, board.opp);
    if moves == 0 {
        if legal_moves(board.opp, board.me) == 0 {
            return Some(terminal_score(final_score(board)));
        }
        return negamax(pass(board), depth, -beta, -alpha, ctx).map(|value| -value);
    }
    if depth == 0 {
        return Some((ctx.config.eval)(board));
    }
    let mut best = -INF;
    let (order, len) = ordered_moves(board, moves, depth, None);
    for &square in &order[..len] {
        let value = -negamax(play(board, square), depth - 1, -beta, -alpha, ctx)?;
        best = best.max(value);
        alpha = alpha.max(value);
        if alpha >= beta {
            break;
        }
    }
    Some(best)
}

pub fn search(board: Board, depth: u32, config: &SearchConfig) -> SearchResult {
    let (moves, first, fallback) = root_setup(board, config);
    if depth == 0 {
        return fallback;
    }
    let mut ctx = Ctx::new(config);
    let (best, score) = root(board, moves, depth, first, &mut ctx).unwrap();
    SearchResult::new(best, score, depth, ctx.nodes, SearchStatus::Complete)
}

/// Deepens to `max_depth`; on tied root scores it keeps the previous best,
/// unlike `search`, which keeps the lowest square.
pub fn search_iterative(board: Board, max_depth: u32, config: &SearchConfig) -> SearchResult {
    search_limited(board, SearchLimits::depth(max_depth), config)
}

pub fn search_limited(board: Board, limits: SearchLimits, config: &SearchConfig) -> SearchResult {
    let deadline = Instant::now().checked_add(Duration::from_millis(limits.time_ms));
    let (moves, fallback, fallback_result) = root_setup(board, config);
    let mut best = fallback;
    let mut nodes = 0;
    let mut result = fallback_result;
    for depth in 1..=limits.max_depth {
        let mut ctx = Ctx {
            config,
            nodes,
            max_nodes: limits.max_nodes,
            deadline,
        };
        if ctx.stop_iteration() {
            break;
        }
        let Some((square, score)) = root(board, moves, depth, best, &mut ctx) else {
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

fn root_setup(board: Board, config: &SearchConfig) -> (u64, u8, SearchResult) {
    let moves = legal_moves(board.me, board.opp);
    assert!(moves != 0, "search requires a legal root move");
    let first = moves.trailing_zeros() as u8;
    let fallback = SearchResult::new(first, (config.eval)(board), 0, 0, SearchStatus::Fallback);
    (moves, first, fallback)
}

fn root(board: Board, moves: u64, depth: u32, best: u8, ctx: &mut Ctx) -> Option<(u8, i32)> {
    let mut chosen = best;
    let mut score = -INF;
    let mut alpha = -INF;
    let (order, len) = ordered_moves(board, moves, depth, Some(best));
    for &square in &order[..len] {
        let value = -negamax(play(board, square), depth - 1, -INF, -alpha, ctx)?;
        if value > score {
            score = value;
            chosen = square;
        }
        alpha = alpha.max(value);
    }
    Some((chosen, score))
}
