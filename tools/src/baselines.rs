use othello::board::{Board, Dir, disc_count, legal_moves, shift};
use othello::score::EVAL_BOUND;
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

const CLASSIC_WEIGHTS: [[i32; 8]; 8] = [
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
const MOBILITY_SCALE: i32 = 1000;
const MOBILITY_WEIGHTS: [i32; 2] = [500, 800];
const FRONTIER_WEIGHTS: [i32; 2] = [10, 4];
const RISK_WEIGHTS: [i32; 2] = [1000, 700];

fn classic_phase(empties: u32) -> i32 {
    let empty = empties.clamp(12, 60);
    (60 - empty) as i32 * 1000 / 48
}

fn classic_blend(weights: [i32; 2], phase: i32) -> i32 {
    weights[0] + (weights[1] - weights[0]) * phase / 1000
}

fn classic_neighbours(bits: u64) -> u64 {
    Dir::ALL.iter().fold(0, |acc, &dir| acc | shift(bits, dir))
}

fn classic_mobility(board: Board) -> i32 {
    let mine = legal_moves(board.me, board.opp).count_ones() as i32;
    let theirs = legal_moves(board.opp, board.me).count_ones() as i32;
    (mine - theirs) * MOBILITY_SCALE / (mine + theirs + 1)
}

fn classic_frontier(board: Board) -> i32 {
    let edge = classic_neighbours(!(board.me | board.opp));
    (edge & board.opp).count_ones() as i32 - (edge & board.me).count_ones() as i32
}

fn classic_corner(square: u8) -> u64 {
    let file = square % 8;
    let rank = square / 8;
    let corner = (if file < 4 { 0 } else { 7 }) + if rank < 4 { 0 } else { 56 };
    1 << corner
}

fn classic_corner_risk(board: Board, bits: u64) -> i32 {
    let occupied = board.me | board.opp;
    let mut risk = 0;
    for (squares, penalty) in [(X_SQUARES, X_PENALTY), (C_SQUARES, C_PENALTY)] {
        let mut owned = bits & squares;
        while owned != 0 {
            let square = owned.trailing_zeros() as u8;
            if occupied & classic_corner(square) == 0 {
                risk += penalty;
            }
            owned &= owned - 1;
        }
    }
    risk
}

pub fn eval_classic(board: Board) -> i32 {
    let (mine, theirs) = (disc_count(board.me) as i32, disc_count(board.opp) as i32);
    let empties = 64 - mine - theirs;
    let phase = classic_phase(empties as u32);
    let mut value = 0;
    for (square, weight) in CLASSIC_WEIGHTS.as_flattened().iter().enumerate() {
        let bit = 1u64 << square;
        if board.me & bit != 0 {
            value += weight;
        } else if board.opp & bit != 0 {
            value -= weight;
        }
    }
    value += classic_mobility(board) * classic_blend(MOBILITY_WEIGHTS, phase) / 1000;
    value += classic_frontier(board) * classic_blend(FRONTIER_WEIGHTS, phase);
    let risk = classic_corner_risk(board, board.opp) - classic_corner_risk(board, board.me);
    value += risk * classic_blend(RISK_WEIGHTS, phase) / 1000;
    value.clamp(-EVAL_BOUND, EVAL_BOUND)
}

pub fn classic_config() -> SearchConfig {
    SearchConfig {
        eval: eval_classic,
        endgame_threshold: 14,
        ordering: true,
    }
}
