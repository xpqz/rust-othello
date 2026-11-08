//! Minimal Phase 2 evaluation in centidiscs.
//!
//! See [`crate::score`] for the heuristic-bound tradeoff.

use crate::board::{Board, Dir, disc_count, legal_moves, shift};
use crate::score::{EVAL_BOUND, SCORE_SCALE};

const WEIGHTS: [[i32; 8]; 8] = [
    [100, -1, 10, 5, 5, 10, -1, 100],
    [-1, -1, -2, -2, -2, -2, -1, -1],
    [10, -2, -1, -1, -1, -1, -2, 10],
    [5, -2, -1, -1, -1, -1, -2, 5],
    [5, -2, -1, -1, -1, -1, -2, 5],
    [10, -2, -1, -1, -1, -1, -2, 10],
    [-1, -1, -2, -2, -2, -2, -1, -1],
    [100, -1, 10, 5, 5, 10, -1, 100],
];

const X_SQUARES: u64 = (1 << 9) | (1 << 14) | (1 << 49) | (1 << 54);
const C_SQUARES: u64 =
    (1 << 1) | (1 << 6) | (1 << 8) | (1 << 15) | (1 << 48) | (1 << 55) | (1 << 57) | (1 << 62);
const X_PENALTY: i32 = 49;
const C_PENALTY: i32 = 19;

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

fn corner(square: u8) -> u64 {
    let file = square % 8;
    let rank = square / 8;
    let corner = (if file < 4 { 0 } else { 7 }) + if rank < 4 { 0 } else { 56 };
    1 << corner
}

fn corner_risk(board: Board, bits: u64) -> i32 {
    let occupied = board.me | board.opp;
    let mut risk = 0;
    for (squares, penalty) in [(X_SQUARES, X_PENALTY), (C_SQUARES, C_PENALTY)] {
        let mut owned = bits & squares;
        while owned != 0 {
            let square = owned.trailing_zeros() as u8;
            if occupied & corner(square) == 0 {
                risk += penalty;
            }
            owned &= owned - 1;
        }
    }
    risk
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
    value -= corner_risk(board, board.me);
    value += corner_risk(board, board.opp);
    value.clamp(-EVAL_BOUND, EVAL_BOUND)
}
