//! Cost of the heuristic fallback that replaces an unfinished exact solve:
//! each position of a recorded match with N empties is searched with the
//! threshold at N and an expired move time, so the win/draw/loss pass stops
//! at its first deadline check and the fallback search decides the move.
//! Prints the time of each call and, per N, the mean and maximum.

use std::time::Instant;

use othello::board::{Board, empty_count, legal_moves, pass, play};
use othello::search::{SearchConfig, SearchLimits, SearchStatus, search_tt};
use othello::tt::{MAX_ENTRIES, Table};

const SOURCE: &str = "docs/tuning/speed_135_match.journal";

fn positions(text: &str, empties: u32) -> Vec<Board> {
    let mut boards = Vec::new();
    for (_, moves) in text.lines().filter_map(|line| line.split_once(" moves ")) {
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
    let path = format!("{}/../{SOURCE}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).unwrap();
    let mut table = Table::new(MAX_ENTRIES);
    println!("fallback_cost v1 positions from {SOURCE}, move time 0 ms, table cleared before each");
    for empties in [14, 16, 18, 20] {
        let config = SearchConfig {
            endgame_threshold: empties,
            ..SearchConfig::standard()
        };
        let (mut times, mut fallbacks) = (Vec::new(), 0);
        for (index, board) in positions(&text, empties).into_iter().enumerate() {
            table.clear();
            let start = Instant::now();
            let result = search_tt(board, SearchLimits::new(64, 0, 0), &config, &mut table);
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            let heuristic = result.status == SearchStatus::Partial && result.depth == 8;
            fallbacks += usize::from(heuristic);
            println!(
                "pos {index} empties {empties} ms {ms:.2} status {:?} depth {} solved {} nodes {}",
                result.status, result.depth, result.solved as u8, result.nodes
            );
            times.push(ms);
        }
        let max = times.iter().copied().fold(0.0, f64::max);
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        println!(
            "empties {empties} positions {} depth8_fallbacks {fallbacks} mean_ms {mean:.2} max_ms {max:.2}",
            times.len()
        );
    }
}
