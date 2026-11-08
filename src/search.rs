//! Fixed-depth negamax with alpha-beta and the search result contract.

use crate::board::{Board, final_score, legal_moves, pass, play};
use crate::score::{INF, terminal_score};

#[derive(Clone, Copy, Debug)]
pub struct SearchConfig {
    pub eval: fn(Board) -> i32,
    pub endgame_threshold: u32,
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
    let mut rest = moves;
    while rest != 0 {
        let square = rest.trailing_zeros() as u8;
        rest &= rest - 1;
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
        return SearchResult {
            best: first,
            score: (config.eval)(board),
            depth: 0,
            nodes: 0,
            status: SearchStatus::Fallback,
            solved: false,
        };
    }
    let mut ctx = Ctx::new(config);
    let mut best = first;
    let mut score = -INF;
    let mut alpha = -INF;
    let mut rest = moves;
    while rest != 0 {
        let square = rest.trailing_zeros() as u8;
        rest &= rest - 1;
        let value = -negamax(play(board, square), depth - 1, -INF, -alpha, &mut ctx).unwrap();
        if value > score {
            score = value;
            best = square;
        }
        alpha = alpha.max(value);
    }
    SearchResult {
        best,
        score,
        depth,
        nodes: ctx.nodes,
        status: SearchStatus::Complete,
        solved: false,
    }
}
