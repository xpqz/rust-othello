use othello::board::{Board, disc_count};
use othello::search::SearchConfig;

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

pub const PHASE2_DEPTH: u32 = 4;

pub fn eval_phase2(board: Board) -> i32 {
    let mut value = (disc_count(board.me) as i32 - disc_count(board.opp) as i32) * 100;
    for (square, weight) in WEIGHTS.as_flattened().iter().enumerate() {
        let bit = 1u64 << square;
        if board.me & bit != 0 {
            value += weight;
        } else if board.opp & bit != 0 {
            value -= weight;
        }
    }
    value.clamp(-6400, 6400)
}

pub fn phase2_config() -> SearchConfig {
    SearchConfig {
        eval: eval_phase2,
        endgame_threshold: 12,
        ordering: true,
    }
}
