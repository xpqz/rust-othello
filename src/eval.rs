//! Minimal Phase 2 evaluation in centidiscs.
//!
//! A heuristic may outrank a small proven win; exact terminal values are never
//! approximated. Dynamic corner risk and the full feature set arrive later.

use crate::board::{Board, disc_count};
use crate::score::{EVAL_BOUND, SCORE_SCALE};

const WEIGHTS: [i32; 64] = [
    100, -20, 10, 5, 5, 10, -20, 100, -20, -50, -2, -2, -2, -2, -50, -20, 10, -2, -1, -1, -1, -1,
    -2, 10, 5, -2, -1, -1, -1, -1, -2, 5, 5, -2, -1, -1, -1, -1, -2, 5, 10, -2, -1, -1, -1, -1, -2,
    10, -20, -50, -2, -2, -2, -2, -50, -20, 100, -20, 10, 5, 5, 10, -20, 100,
];

/// Positional table plus disc difference, in centidiscs.
pub fn evaluate(board: Board) -> i32 {
    let mut value = (disc_count(board.me) as i32 - disc_count(board.opp) as i32) * SCORE_SCALE;
    for (square, weight) in WEIGHTS.iter().enumerate() {
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
