use std::time::Instant;

use othello::board::{Board, legal_moves};
use othello::pattern;
use othello::search::{SearchConfig, SearchLimits, search_limited};
use othello_tools::baselines::classic_config;

fn main() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/corpus.txt"))
        .unwrap();
    let boards: Vec<Board> = text
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let me = u64::from_str_radix(parts.next()?, 16).ok()?;
            let opp = u64::from_str_radix(parts.next()?, 16).ok()?;
            Some(Board { me, opp })
        })
        .collect();
    let pattern = SearchConfig {
        eval: pattern::evaluate,
        endgame_threshold: 14,
        ordering: true,
    };
    for (label, config) in [("classic", classic_config()), ("pattern", pattern)] {
        let mut nps = Vec::new();
        let mut depths = Vec::new();
        for (index, &board) in boards.iter().enumerate() {
            if legal_moves(board.me, board.opp) == 0 {
                println!("{label} {index} skip");
                continue;
            }
            let start = Instant::now();
            let result = search_limited(board, SearchLimits::new(64, 3000, 0), &config);
            let elapsed = start.elapsed().as_secs_f64();
            let rate = result.nodes as f64 / elapsed.max(f64::EPSILON);
            nps.push(rate);
            depths.push(result.depth);
            println!(
                "{label} {index} nodes {} depth {} seconds {elapsed:.3} nps {rate:.0}",
                result.nodes, result.depth
            );
        }
        let median = |values: &mut Vec<f64>| {
            values.sort_by(|a, b| a.total_cmp(b));
            if values.is_empty() {
                0.0
            } else {
                values[values.len() / 2]
            }
        };
        let mut sorted_depths: Vec<f64> = depths.iter().map(|&depth| depth as f64).collect();
        println!(
            "{label} positions {} median_nps {:.0} median_depth {:.0}",
            nps.len(),
            median(&mut nps),
            median(&mut sorted_depths)
        );
    }
}
