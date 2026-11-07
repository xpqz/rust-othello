//! Centidisc score units and the WTHOR terminal score.
//!
//! The objective is to maximise the WTHOR final score: the disc difference
//! with empty squares awarded to the winner. Its sign encodes the outcome and
//! its magnitude the margin. Heuristic values estimate the same quantity.
//!
//! A heuristic value may outrank a small proven win. Exact terminal values are
//! never approximated.

/// Centidiscs per disc.
pub const SCORE_SCALE: i32 = 100;

/// Score outside the range of any real position.
pub const INF: i32 = 1_000_000;

/// Maximum absolute heuristic score. It equals the largest exact score, so a
/// clamped heuristic can tie a 64-0 win.
pub const EVAL_BOUND: i32 = 6_400;

/// Converts a WTHOR disc difference to centidiscs.
pub fn terminal_score(wthor_diff: i32) -> i32 {
    wthor_diff * SCORE_SCALE
}
