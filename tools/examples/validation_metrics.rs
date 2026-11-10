use othello::board::{Board, play};
use othello::search::{SearchConfig, SearchLimits, probcut_at, search_limited};
use othello_tools::positions;

fn median(values: &mut [u32]) -> u32 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn main() {
    let openings = positions::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bench/openings_validation.txt"
    ))
    .unwrap();
    let configs = [
        (
            "off",
            SearchConfig {
                probcut: None,
                ..SearchConfig::standard()
            },
        ),
        (
            "on",
            SearchConfig {
                probcut: Some(probcut_at(100).unwrap()),
                ..SearchConfig::standard()
            },
        ),
    ];
    for (label, config) in configs {
        let (mut depths, mut tries, mut cuts) = (Vec::new(), 0u64, 0u64);
        for opening in openings.iter().take(50) {
            let mut board = Board::START;
            for &square in &opening.squares {
                board = play(board, square);
            }
            let result = search_limited(board, SearchLimits::new(64, 3000, 0), &config);
            depths.push(result.depth);
            tries += result.probcut_tries.iter().sum::<u64>();
            cuts += result.probcut_cuts.iter().sum::<u64>();
        }
        let mean = depths.iter().sum::<u32>() as f64 / depths.len() as f64;
        println!(
            "{label} mean_depth {mean:.2} median_depth {} tries_per_move {:.2} cuts_per_move {:.2}",
            median(&mut depths),
            tries as f64 / 50.0,
            cuts as f64 / 50.0
        );
    }
}
