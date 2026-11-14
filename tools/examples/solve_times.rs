//! Time to solve, per position, at several empties counts, on the positions of
//! a recorded match: from each game, the first position with exactly N empties
//! and a legal move, solved with the threshold set to N under the move budget
//! a match uses. Cold: table cleared before the solve. In game: the same side's
//! previous move is searched first on the same table, as a match worker would,
//! and the solve follows without clearing. Prints every position and, per
//! condition, the mean, 95th and 99th percentiles and maximum.
//! usage: solve_times [--empties 16,18,20] [--budget 30000] [--previous 3000]

use std::time::Instant;

use othello::board::{Board, empty_count, legal_moves, pass, play};
use othello::search::{SearchConfig, SearchLimits, search_tt};
use othello::tt::{MAX_ENTRIES, Table};

const JOURNAL: &str = "docs/tuning/speed_135_match.journal";

/// Every position of each game with the colour to move, passes included.
fn games(text: &str) -> Vec<Vec<(Board, bool)>> {
    let lines = text.lines().filter_map(|line| line.split_once(" moves "));
    let mut games = Vec::new();
    for (_, moves) in lines {
        let (mut board, mut black) = (Board::START, true);
        let mut positions = vec![(board, black)];
        for token in moves.split_whitespace() {
            board = match token.as_bytes() {
                b"pass" => pass(board),
                &[file, rank] => play(board, (rank - b'1') * 8 + (file - b'a')),
                _ => panic!("bad move {token}"),
            };
            black = !black;
            positions.push((board, black));
        }
        games.push(positions);
    }
    games
}

fn summary(label: &str, empties: u32, times: &mut [f64], unsolved: usize) {
    times.sort_by(|a, b| a.total_cmp(b));
    let at = |fraction: f64| {
        times[((times.len() as f64 * fraction).ceil() as usize).min(times.len()) - 1]
    };
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    println!(
        "{label} empties {empties} positions {} mean_ms {mean:.1} p95_ms {:.1} p99_ms {:.1} max_ms {:.1} unsolved {unsolved}",
        times.len(),
        at(0.95),
        at(0.99),
        times[times.len() - 1]
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str, default: &str| {
        let at = args.iter().position(|arg| arg == flag);
        at.and_then(|at| args.get(at + 1))
            .map_or(default.to_string(), String::clone)
    };
    let list: Vec<u32> = value("--empties", "16,18,20")
        .split(',')
        .map(|n| n.parse().expect("--empties"))
        .collect();
    let budget: u64 = value("--budget", "30000").parse().expect("--budget");
    let previous: u64 = value("--previous", "3000").parse().expect("--previous");
    let path = format!("{}/../{JOURNAL}", env!("CARGO_MANIFEST_DIR"));
    let games = games(&std::fs::read_to_string(path).unwrap());
    let mut table = Table::new(MAX_ENTRIES);
    println!("solve_times v1 positions from {JOURNAL} budget_ms {budget} previous_ms {previous}");
    for empties in list {
        let config = SearchConfig {
            endgame_threshold: empties,
            ..SearchConfig::standard()
        };
        let solve = |table: &mut Table, board: Board| {
            let start = Instant::now();
            let limits = SearchLimits::new(64, budget, 0);
            let result = search_tt(board, limits, &config, table);
            (start.elapsed().as_secs_f64() * 1000.0, result)
        };
        let (mut cold, mut warm, mut unsolved) = (Vec::new(), Vec::new(), [0, 0]);
        for (game, positions) in games.iter().enumerate() {
            let found = positions.iter().position(|(board, _)| {
                empty_count(*board) == empties && legal_moves(board.me, board.opp) != 0
            });
            let Some(at) = found else { continue };
            let (board, black) = positions[at];
            table.clear();
            let (cold_ms, cold_result) = solve(&mut table, board);
            let earlier = positions[..at].iter().rposition(|&(prior, colour)| {
                colour == black && legal_moves(prior.me, prior.opp) != 0
            });
            table.clear();
            if let Some(earlier) = earlier {
                let limits = SearchLimits::new(64, previous, 0);
                search_tt(positions[earlier].0, limits, &config, &mut table);
            }
            let (warm_ms, warm_result) = solve(&mut table, board);
            if cold_result.solved && warm_result.solved {
                assert_eq!(cold_result.score, warm_result.score, "{board:?}");
            }
            unsolved[0] += usize::from(!cold_result.solved);
            unsolved[1] += usize::from(!warm_result.solved);
            println!(
                "game {game} empties {empties} board {:016x} {:016x} cold_ms {cold_ms:.1} cold_solved {} in_game_ms {warm_ms:.1} in_game_solved {} score {}",
                board.me,
                board.opp,
                cold_result.solved as u8,
                warm_result.solved as u8,
                cold_result.score
            );
            cold.push(cold_ms);
            warm.push(warm_ms);
        }
        summary("cold", empties, &mut cold, unsolved[0]);
        summary("in_game", empties, &mut warm, unsolved[1]);
    }
}
