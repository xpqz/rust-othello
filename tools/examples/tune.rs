use othello::agents::search_agent;
use othello::board::{Board, disc_count, is_terminal, legal_moves, pass, play, squares};
use othello::eval::{frontier, mobility, phase};
use othello::rng::Rng;
use othello::search::{NO_DEADLINE, SearchConfig, SearchLimits};
use othello_tools::harness::{GameLog, play_game, points};
use othello_tools::positions;
use othello_tools::stats::{RESAMPLES, report_with};

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
const X: u64 = (1 << 9) | (1 << 14) | (1 << 49) | (1 << 54);
const C: u64 =
    (1 << 1) | (1 << 6) | (1 << 8) | (1 << 15) | (1 << 48) | (1 << 55) | (1 << 57) | (1 << 62);

#[derive(Clone, Copy)]
struct Eval {
    mobility: [i32; 2],
    frontier: [i32; 2],
    risk: [i32; 2],
    disc: bool,
}

const BASE: Eval = Eval {
    mobility: [500, 800],
    frontier: [10, 4],
    risk: [1000, 700],
    disc: true,
};
const NO_MOBILITY: Eval = Eval {
    mobility: [0, 0],
    ..BASE
};
const NO_FRONTIER: Eval = Eval {
    frontier: [0, 0],
    ..BASE
};
const NO_RISK: Eval = Eval {
    risk: [0, 0],
    ..BASE
};
const NO_DISC: Eval = Eval {
    disc: false,
    ..BASE
};

fn corner(square: u8) -> u64 {
    let file = square % 8;
    let rank = square / 8;
    1 << ((if file < 4 { 0 } else { 7 }) + if rank < 4 { 0 } else { 56 })
}

fn risk(board: Board, bits: u64) -> i32 {
    let occupied = board.me | board.opp;
    let mut risk = 0;
    for (squares, penalty) in [(X, 49), (C, 19)] {
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

fn disc_weight(empties: u32) -> i32 {
    phase(empties) / 10
}

fn blend(weights: [i32; 2], phase: i32) -> i32 {
    weights[0] + (weights[1] - weights[0]) * phase / 1000
}

fn eval(board: Board, weights: Eval) -> i32 {
    let (mine, theirs) = (disc_count(board.me) as i32, disc_count(board.opp) as i32);
    let empties = (64 - mine - theirs) as u32;
    let phase = phase(empties);
    let mut value = 0;
    for (square, weight) in WEIGHTS.as_flattened().iter().enumerate() {
        let bit = 1u64 << square;
        if board.me & bit != 0 {
            value += weight;
        } else if board.opp & bit != 0 {
            value -= weight;
        }
    }
    value += mobility(board) * blend(weights.mobility, phase) / 1000;
    value += frontier(board) * blend(weights.frontier, phase);
    value += (risk(board, board.opp) - risk(board, board.me)) * blend(weights.risk, phase) / 1000;
    if weights.disc {
        value += (mine - theirs) * disc_weight(empties);
    }
    value.clamp(-6400, 6400)
}

fn base(board: Board) -> i32 {
    eval(board, BASE)
}
fn no_mobility(board: Board) -> i32 {
    eval(board, NO_MOBILITY)
}
fn no_frontier(board: Board) -> i32 {
    eval(board, NO_FRONTIER)
}
fn no_risk(board: Board) -> i32 {
    eval(board, NO_RISK)
}
fn no_disc(board: Board) -> i32 {
    eval(board, NO_DISC)
}

fn stream(seed: u64, pair: usize, game: u64, side: u64) -> u64 {
    Rng::new(Rng::new(seed).next_u64() ^ ((pair as u64) << 2 | (game << 1) | side)).next_u64()
}

fn check_engine() {
    let mut rng = Rng::new(0xC0DE);
    let mut board = Board::START;
    let mut checked = 0;
    while checked < 1000 {
        if is_terminal(board) {
            board = Board::START;
            continue;
        }
        let legal = legal_moves(board.me, board.opp);
        if legal == 0 {
            board = pass(board);
            continue;
        }
        assert_eq!(eval(board, NO_DISC), othello::eval::evaluate(board));
        checked += 1;
        let pick = (rng.next_u64() % legal.count_ones() as u64) as usize;
        board = play(board, squares(legal).nth(pick).unwrap());
    }
}

fn main() {
    check_engine();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let variant = args.first().map(String::as_str).unwrap_or("base");
    let pairs: usize = args.get(1).map_or(400, |s| s.parse().unwrap());
    let seed: u64 = args.get(2).map_or(43, |s| s.parse().unwrap());
    let nodes: u64 = args.get(3).map_or(2000, |s| s.parse().unwrap());
    let path = args.get(4).map_or(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/openings_tune.txt"),
        String::as_str,
    );
    let shown = args
        .get(4)
        .map_or("bench/openings_tune.txt", String::as_str);
    let (label, eval): (&str, fn(Board) -> i32) = match variant {
        "base" => ("base", base),
        "no-mobility" => ("no-mobility", no_mobility),
        "no-frontier" => ("no-frontier", no_frontier),
        "no-risk" => ("no-risk", no_risk),
        "no-disc" => ("no-disc", no_disc),
        _ => panic!("unknown variant"),
    };
    let openings = positions::load(path).unwrap();
    let limits = SearchLimits::new(64, NO_DEADLINE, nodes);
    let candidate = SearchConfig {
        eval,
        ..SearchConfig::standard()
    };
    let baseline = SearchConfig {
        eval: base,
        ..SearchConfig::standard()
    };
    let mut game_points = Vec::with_capacity(pairs);
    let mut logs = Vec::new();
    for (pair, opening) in openings.iter().enumerate().take(pairs) {
        let mut pair_points = [0.0, 0.0];
        for (game, slot) in pair_points.iter_mut().enumerate() {
            let candidate_black = game == 0;
            let black_seed = stream(seed, pair, game as u64, 0);
            let white_seed = stream(seed, pair, game as u64, 1);
            let config = |black: bool| {
                if black == candidate_black {
                    candidate
                } else {
                    baseline
                }
            };
            let mut black_agent = search_agent(config(true), limits);
            let mut white_agent = search_agent(config(false), limits);
            let result = play_game(&opening.squares, &mut black_agent, &mut white_agent).unwrap();
            *slot = if game == 0 {
                points(result.score)
            } else {
                1.0 - points(result.score)
            };
            logs.push(
                GameLog {
                    run: seed,
                    pair,
                    game: game as u64,
                    agent_seeds: (black_seed, white_seed),
                    key: opening.key,
                    opening: opening.line(),
                    black: if candidate_black { label } else { "base" }.into(),
                    white: if candidate_black { "base" } else { label }.into(),
                    score: result.score,
                }
                .line(),
            );
        }
        game_points.push((pair_points[0], pair_points[1]));
    }
    let summary = report_with(&game_points, RESAMPLES, seed);
    let interval = summary.interval.bounds().unwrap();
    println!(
        "{} vs base pairs {} games {} W {} D {} L {} mean {:.4} interval [{:.4}, {:.4}]",
        label,
        summary.pairs,
        summary.games,
        summary.wins,
        summary.draws,
        summary.losses,
        summary.mean,
        interval.0,
        interval.1
    );
    println!("seed {seed} nodes {nodes} openings {shown}");
    for line in logs {
        println!("{line}");
    }
}
