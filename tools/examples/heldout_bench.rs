//! Search time at several depths on positions that are not the benchmark's:
//! tune openings continued by pseudo-random legal moves, so the positions do
//! not depend on the engine being measured. Cold table per search, clearing
//! not timed. Prints every position's score so two builds can be compared
//! line by line.
//! usage: heldout_bench [--pick 0..7] [--depths 9,10,11] [--passes 2]

use std::time::Instant;

use othello::board::{Board, empty_count, legal_moves, pass, play};
use othello::search::{SearchConfig, SearchLimits, search_tt};
use othello::tt::{MAX_ENTRIES, Table};
use othello_tools::positions::load;

const SEED: u64 = 0x139;

/// Every eighth tune opening, played on by a fixed generator to 30 to 40 empties.
fn positions(pick: usize) -> Vec<Board> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/openings_tune.txt");
    let openings = load(path).unwrap();
    let mut boards = Vec::new();
    for (line, opening) in openings
        .iter()
        .enumerate()
        .filter(|(line, _)| line % 8 == pick)
    {
        let mut board = Board::START;
        for &square in &opening.squares {
            board = play(board, square);
        }
        let target = 30 + (boards.len() as u32 % 11);
        let mut state = SEED ^ line as u64;
        while empty_count(board) > target {
            let moves = legal_moves(board.me, board.opp);
            if moves == 0 {
                if legal_moves(board.opp, board.me) == 0 {
                    break;
                }
                board = pass(board);
                continue;
            }
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let mut rest = moves;
            for _ in 0..(state >> 33) % u64::from(moves.count_ones()) {
                rest &= rest - 1;
            }
            board = play(board, rest.trailing_zeros() as u8);
        }
        if empty_count(board) == target && legal_moves(board.me, board.opp) != 0 {
            boards.push(board);
        }
    }
    boards
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str, default: &str| {
        let at = args.iter().position(|arg| arg == flag);
        at.and_then(|at| args.get(at + 1))
            .map_or(default.to_string(), String::clone)
    };
    let pick: usize = value("--pick", "3").parse().expect("--pick");
    let passes: usize = value("--passes", "2").parse().expect("--passes");
    let depths: Vec<u32> = value("--depths", "9,10,11")
        .split(',')
        .map(|depth| depth.parse().expect("--depths"))
        .collect();
    let boards = positions(pick);
    let config = SearchConfig::standard();
    let mut table = Table::new(MAX_ENTRIES);
    println!(
        "heldout v1 openings bench/openings_tune.txt pick {pick} seed {SEED:#x} positions {} passes {passes}",
        boards.len()
    );
    for depth in depths {
        let (mut nodes, mut best) = (0, f64::MAX);
        for round in 0..passes {
            let mut time = 0.0;
            nodes = 0;
            for (index, &board) in boards.iter().enumerate() {
                table.clear();
                let start = Instant::now();
                let result = search_tt(board, SearchLimits::depth(depth), &config, &mut table);
                time += start.elapsed().as_secs_f64() * 1000.0;
                nodes += result.nodes;
                if round == 0 {
                    println!(
                        "pos {index} depth {depth} board {:016x} {:016x} score {}",
                        board.me, board.opp, result.score
                    );
                }
            }
            println!("round {round} depth {depth} nodes {nodes} ms {time:.1}");
            best = best.min(time);
        }
        println!("depth {depth} nodes {nodes} min_ms {best:.1}");
    }
}
