//! Minimal Phase 2 evaluation in centidiscs.
//!
//! See [`crate::score`] for the heuristic-bound tradeoff.

use crate::board::{Board, Dir, disc_count, legal_moves, shift};
use crate::score::{EVAL_BOUND, SCORE_SCALE};

const WEIGHTS: [[i32; 8]; 8] = [
    [100, -20, 10, 5, 5, 10, -20, 100],
    [-20, -50, -2, -2, -2, -2, -50, -20],
    [10, -2, -1, -1, -1, -1, -2, 10],
    [5, -2, -1, -1, -1, -1, -2, 5],
    [5, -2, -1, -1, -1, -1, -2, 5],
    [10, -2, -1, -1, -1, -1, -2, 10],
    [-20, -50, -2, -2, -2, -2, -50, -20],
    [100, -20, 10, 5, 5, 10, -20, 100],
];

pub const MOBILITY_SCALE: i32 = 1000;

fn neighbours(bits: u64) -> u64 {
    Dir::ALL.iter().fold(0, |acc, &dir| acc | shift(bits, dir))
}

pub fn mobility(board: Board) -> i32 {
    let mine = legal_moves(board.me, board.opp).count_ones() as i32;
    let theirs = legal_moves(board.opp, board.me).count_ones() as i32;
    (mine - theirs) * MOBILITY_SCALE / (mine + theirs + 1)
}

pub fn frontier(board: Board) -> i32 {
    let edge = neighbours(!(board.me | board.opp));
    (edge & board.opp).count_ones() as i32 - (edge & board.me).count_ones() as i32
}

/// Positional table plus disc difference, in centidiscs.
///
/// For non-terminal positions; terminals use
/// [`crate::score::terminal_score`].
pub fn evaluate(board: Board) -> i32 {
    let mut value = (disc_count(board.me) as i32 - disc_count(board.opp) as i32) * SCORE_SCALE;
    for (square, weight) in WEIGHTS.as_flattened().iter().enumerate() {
        let bit = 1u64 << square;
        if board.me & bit != 0 {
            value += weight;
        } else if board.opp & bit != 0 {
            value -= weight;
        }
    }
    debug_assert!(value.abs() <= EVAL_BOUND, "evaluate out of bounds: {value}");
    value.clamp(-EVAL_BOUND, EVAL_BOUND)
}
