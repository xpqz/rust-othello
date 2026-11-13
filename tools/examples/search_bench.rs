//! Search time for a fixed, fully completed workload on the corpus: midgame
//! positions to a fixed depth and exact solves, cold table each search,
//! several rounds. Clearing the table is timed apart and not counted: on the
//! exact workload it is about half the wall time and is memory traffic, not
//! search. Node totals are printed so runs can be compared.

use std::time::Instant;

use othello::board::{Board, empty_count, legal_moves};
use othello::search::{SearchConfig, SearchLimits, search_tt};
use othello::tt::{MAX_ENTRIES, Table};

const ROUNDS: usize = 5;
const MIDGAME_DEPTH: u32 = 10;
const SOLVE_REPEATS: usize = 100;

fn workload(
    boards: &[Board],
    limits: SearchLimits,
    repeats: usize,
    table: &mut Table,
) -> (u64, f64, f64) {
    let config = SearchConfig::standard();
    let (mut nodes, mut search, mut clear) = (0, 0.0, 0.0);
    for _ in 0..repeats {
        for &board in boards {
            let start = Instant::now();
            table.clear();
            let cleared = Instant::now();
            nodes += search_tt(board, limits, &config, table).nodes;
            clear += (cleared - start).as_secs_f64() * 1000.0;
            search += cleared.elapsed().as_secs_f64() * 1000.0;
        }
    }
    (nodes, search, clear)
}

fn summary(label: &str, rounds: &[(u64, f64, f64)]) {
    let mut times: Vec<f64> = rounds.iter().map(|round| round.1).collect();
    times.sort_by(|a, b| a.total_cmp(b));
    let (min, max) = (times[0], times[times.len() - 1]);
    assert!(
        rounds.iter().all(|round| round.0 == rounds[0].0),
        "{label} nodes vary"
    );
    println!(
        "{label} nodes {} min_ms {min:.1} median_ms {:.1} max_ms {max:.1} spread {:.4} nodes_per_s {:.0}",
        rounds[0].0,
        times[times.len() / 2],
        (max - min) / min,
        rounds[0].0 as f64 / (min / 1000.0)
    );
}

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
        .filter(|board| legal_moves(board.me, board.opp) != 0)
        .collect();
    let threshold = SearchConfig::standard().endgame_threshold;
    let (solves, midgames): (Vec<Board>, Vec<Board>) = boards
        .iter()
        .partition(|board| empty_count(**board) <= threshold);
    println!(
        "bench v2 (search time; table clearing apart) midgame positions {} depth {MIDGAME_DEPTH} exact positions {} repeats {SOLVE_REPEATS} rounds {ROUNDS}",
        midgames.len(),
        solves.len()
    );
    let mut table = Table::new(MAX_ENTRIES);
    let (mut midgame, mut exact) = (Vec::new(), Vec::new());
    for round in 0..ROUNDS {
        let m = workload(&midgames, SearchLimits::depth(MIDGAME_DEPTH), 1, &mut table);
        let e = workload(&solves, SearchLimits::depth(64), SOLVE_REPEATS, &mut table);
        println!(
            "round {round} midgame nodes {} ms {:.1} exact nodes {} ms {:.1} clear ms {:.1} {:.1}",
            m.0, m.1, e.0, e.1, m.2, e.2
        );
        midgame.push(m);
        exact.push(e);
    }
    summary("midgame", &midgame);
    summary("exact", &exact);
}
