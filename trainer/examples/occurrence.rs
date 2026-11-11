use std::collections::BTreeSet;

use othello::board::{Board, empty_count, is_terminal, legal_moves, pass, play};
use othello::pattern;
use othello::score::EVAL_BOUND;
use othello::search::{NO_DEADLINE, SearchConfig, SearchLimits, search_limited};
use othello_trainer::fit;

fn main() {
    let asset = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/eval.bin")).unwrap();
    let counts = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../data/fit/counts.bin"
    ))
    .unwrap();
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/corpus.txt"))
        .unwrap();
    let corpus: Vec<Board> = text
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let me = u64::from_str_radix(parts.next()?, 16).ok()?;
            let opp = u64::from_str_radix(parts.next()?, 16).ok()?;
            Some(Board { me, opp })
        })
        .collect();
    report("corpus", &corpus, &asset, &counts);

    let config = SearchConfig {
        eval: pattern::evaluate,
        endgame_threshold: 14,
        ordering: true,
    };
    let mut sample = BTreeSet::new();
    for &start in corpus.iter().take(10) {
        let mut board = start;
        for _ in 0..20 {
            if is_terminal(board) {
                break;
            }
            let moves = legal_moves(board.me, board.opp);
            if moves == 0 {
                board = pass(board);
                continue;
            }
            sample.insert((board.me, board.opp));
            let limits = SearchLimits::new(64, NO_DEADLINE, 200_000);
            let result = search_limited(board, limits, &config);
            board = play(board, result.best);
        }
    }
    let sample: Vec<Board> = sample.iter().map(|&(me, opp)| Board { me, opp }).collect();
    report("sample", &sample, &asset, &counts);
}

fn report(label: &str, boards: &[Board], asset: &[u8], counts: &[u8]) {
    let mut raw_over = 0;
    let mut under20 = 0;
    let mut entries = 0;
    for &board in boards {
        let stage = pattern::stage(empty_count(board));
        let features = pattern::features(board);
        let mut raw = 0i32;
        for &feature in features.iter() {
            let offset = (stage * pattern::TABLE_SLOTS + feature as usize) * 2;
            raw += i16::from_le_bytes([asset[offset], asset[offset + 1]]) as i32;
            let canonical = fit::canonical(feature) as usize;
            let count_offset = (stage * pattern::TABLE_SLOTS + canonical) * 4;
            let count = u32::from_le_bytes([
                counts[count_offset],
                counts[count_offset + 1],
                counts[count_offset + 2],
                counts[count_offset + 3],
            ]);
            if count < 20 {
                under20 += 1;
            }
            entries += 1;
        }
        if raw.abs() > EVAL_BOUND {
            raw_over += 1;
        }
    }
    println!(
        "{label} positions {} raw_over {raw_over} entries_under20 {under20}/{entries}",
        boards.len()
    );
}
