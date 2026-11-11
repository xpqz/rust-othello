use std::hint::black_box;
use std::time::Instant;

use othello::board::{Board, empty_count};
use othello::pattern;
use othello_tools::baselines::eval_classic;

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
    for stage in 0..pattern::STAGES {
        let stage_boards: Vec<Board> = boards
            .iter()
            .copied()
            .filter(|&board| pattern::stage(empty_count(board)) == stage)
            .collect();
        if stage_boards.is_empty() {
            println!("stage {stage} boards 0 classic_ns 0.0 pattern_ns 0.0");
            continue;
        }
        let iterations = 20_000;
        let start = Instant::now();
        for _ in 0..iterations {
            for &board in &stage_boards {
                black_box(eval_classic(black_box(board)));
            }
        }
        let classic = start.elapsed().as_nanos() as f64 / (iterations * stage_boards.len()) as f64;
        let start = Instant::now();
        for _ in 0..iterations {
            for &board in &stage_boards {
                black_box(pattern::evaluate(black_box(board)));
            }
        }
        let pattern = start.elapsed().as_nanos() as f64 / (iterations * stage_boards.len()) as f64;
        println!(
            "stage {stage} boards {} classic_ns {classic:.1} pattern_ns {pattern:.1}",
            stage_boards.len()
        );
    }
}
