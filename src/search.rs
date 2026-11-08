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
        Self::limited(config, 0, 0, None)
    }

    fn limited(
        config: &'a SearchConfig,
        nodes: u64,
        max_nodes: u64,
        deadline: Option<Instant>,
    ) -> Self {
        Self {
            config,
            nodes,
            max_nodes,
            deadline,
        }
    }

    fn stop(&self) -> bool {
        (self.max_nodes > 0 && self.nodes > self.max_nodes)
            || self
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
    }
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
    for square in squares(moves) {
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
        if ctx.stop() {
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
    for square in std::iter::once(best).chain(squares(moves & !(1u64 << best))) {
        let value = -negamax(play(board, square), depth - 1, -INF, -alpha, ctx)?;
        if value > score {
            score = value;
            chosen = square;
        }
        alpha = alpha.max(value);
    }
    Some((chosen, score))
}
