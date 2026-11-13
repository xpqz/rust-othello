//! Search time for exact solves of positions from real games: from each game
//! of the issue 136 match, the first position with exactly N empties and a
//! legal move, solved with the threshold set to N: all 200 games for N = 14,
//! every second game for N = 16. Cold table each solve, clearing not timed,
//! one untimed warm-up pass, then several rounds. Node totals and every
//! position's score are printed so two builds can be compared.

use std::time::Instant;

use othello::board::{Board, empty_count, legal_moves, pass, play};
use othello::search::{SearchConfig, SearchLimits, search_tt};
use othello::tt::{MAX_ENTRIES, Table};

const ROUNDS: usize = 5;
const JOURNAL: &str = "docs/tuning/speed_135_match.journal";

fn positions(text: &str, empties: u32) -> Vec<Board> {
    let games = text.lines().filter_map(|line| line.split_once(" moves "));
    let mut boards = Vec::new();
    for (_, moves) in games {
        let mut board = Board::START;
        for token in moves.split_whitespace() {
            if empty_count(board) == empties && legal_moves(board.me, board.opp) != 0 {
                boards.push(board);
                break;
            }
            board = match token.as_bytes() {
                b"pass" => pass(board),
                &[file, rank] => play(board, (rank - b'1') * 8 + (file - b'a')),
                _ => panic!("bad move {token}"),
            };
        }
    }
    boards
}

fn main() {
    let path = format!("{}/../{JOURNAL}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let mut table = Table::new(MAX_ENTRIES);
    println!("exact_bench v1 positions from {JOURNAL} rounds {ROUNDS}");
    for (empties, step) in [(14, 1), (16, 2)] {
        let boards: Vec<Board> = positions(&text, empties)
            .into_iter()
            .step_by(step)
            .collect();
        let config = SearchConfig {
            endgame_threshold: empties,
            ..SearchConfig::standard()
        };
        if empties == 14 {
            for &board in &boards {
                table.clear();
                search_tt(board, SearchLimits::depth(64), &config, &mut table);
            }
        }
        let mut times = Vec::new();
        let mut totals = Vec::new();
        for round in 0..ROUNDS {
            let (mut nodes, mut time) = (0, 0.0);
            for (index, &board) in boards.iter().enumerate() {
                table.clear();
                let start = Instant::now();
                let result = search_tt(board, SearchLimits::depth(64), &config, &mut table);
                time += start.elapsed().as_secs_f64() * 1000.0;
                assert!(result.solved, "{board:?}");
                nodes += result.nodes;
                if round == 0 {
                    println!(
                        "pos {index} empties {empties} board {:016x} {:016x} best {} score {}",
                        board.me, board.opp, result.best, result.score
                    );
                }
            }
            println!("round {round} empties {empties} nodes {nodes} ms {time:.1}");
            times.push(time);
            totals.push(nodes);
        }
        assert!(totals.iter().all(|&nodes| nodes == totals[0]), "nodes vary");
        times.sort_by(|a, b| a.total_cmp(b));
        let (min, max) = (times[0], times[ROUNDS - 1]);
        println!(
            "empties{empties} nodes {} min_ms {min:.1} median_ms {:.1} max_ms {max:.1} spread {:.4} nodes_per_s {:.0} positions {}",
            totals[0],
            times[ROUNDS / 2],
            (max - min) / min,
            totals[0] as f64 / (min / 1000.0),
            boards.len()
        );
    }
}
