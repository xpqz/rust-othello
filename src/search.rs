//! Fixed-depth negamax with alpha-beta and the search result contract.

use crate::board::{Board, final_score, legal_moves, pass, play, squares};
use crate::score::{INF, terminal_score};

#[derive(Clone, Copy, Debug)]
pub struct SearchConfig {
    pub eval: fn(Board) -> i32,
    pub endgame_threshold: u32,
}

/// Phase 2 limits; time and node limits arrive with cancellation in #83.
#[derive(Clone, Copy, Debug)]
pub struct SearchLimits {
    pub max_depth: u32,
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
}

impl<'a> Ctx<'a> {
    pub fn new(config: &'a SearchConfig) -> Self {
        Self { config, nodes: 0 }
    }
}

pub fn negamax(board: Board, depth: u32, mut alpha: i32, beta: i32, ctx: &mut Ctx) -> Option<i32> {
    ctx.nodes += 1;
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
    let moves = legal_moves(board.me, board.opp);
    assert!(moves != 0, "search requires a legal root move");
    let first = moves.trailing_zeros() as u8;
    if depth == 0 {
        return SearchResult::new(first, (config.eval)(board), 0, 0, SearchStatus::Fallback);
    }
    let mut ctx = Ctx::new(config);
    let (best, score) = root(board, moves, depth, first, &mut ctx);
    SearchResult::new(best, score, depth, ctx.nodes, SearchStatus::Complete)
}

pub fn search_iterative(board: Board, max_depth: u32, config: &SearchConfig) -> SearchResult {
    let moves = legal_moves(board.me, board.opp);
    assert!(moves != 0, "search requires a legal root move");
    let mut best = moves.trailing_zeros() as u8;
    let mut result = SearchResult::new(best, (config.eval)(board), 0, 0, SearchStatus::Fallback);
    let mut nodes = 0;
    for depth in 1..=max_depth {
        let mut ctx = Ctx::new(config);
        let (square, score) = root(board, moves, depth, best, &mut ctx);
        best = square;
        nodes += ctx.nodes;
        result = SearchResult::new(square, score, depth, nodes, SearchStatus::Complete);
    }
    result
}

fn root(board: Board, moves: u64, depth: u32, best: u8, ctx: &mut Ctx) -> (u8, i32) {
    let mut chosen = best;
    let mut score = -INF;
    let mut alpha = -INF;
    for square in std::iter::once(best).chain(squares(moves & !(1u64 << best))) {
        let value = -negamax(play(board, square), depth - 1, -INF, -alpha, ctx).unwrap();
        if value > score {
            score = value;
            chosen = square;
        }
        alpha = alpha.max(value);
    }
    (chosen, score)
}
