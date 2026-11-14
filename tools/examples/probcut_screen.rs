//! ProbCut v2 screen (docs/design/probcut2.md § Screening and tuning `t`,
//! SKIS #148 step 3), for one candidate `t` in hundredths of σ with the
//! frozen `ProbCut::FITTED` model:
//!
//! - `nodes`: the `search_bench` midgame workload (corpus positions with more
//!   than 14 empties, depth 10, a cleared table per search) with ProbCut and
//!   full-width; passes if ProbCut uses at most 60% of the nodes.
//! - `depth`: the positions after 20 plies of the first 50 games of
//!   `docs/tuning/speed_135_match.journal`, searched for `--ms` each with a
//!   fresh table, ProbCut and full-width; passes if the median completed
//!   depth with ProbCut is at least the full-width median plus one.
//!
//! usage: probcut_screen nodes --t T
//!        probcut_screen depth --t T [--ms 3000] [--jobs 5]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use othello::board::{Board, empty_count, legal_moves, pass, play};
use othello::search::{ProbCut, SearchConfig, SearchLimits, search_tt};
use othello::tt::{MAX_ENTRIES, Table};

const SPLIT_EMPTIES: u32 = 14;
const MIDGAME_DEPTH: u32 = 10;
const GAMES: usize = 50;
const PLIES: usize = 20;

fn configs(t: u32) -> [(&'static str, SearchConfig); 2] {
    let off = SearchConfig {
        endgame_threshold: SPLIT_EMPTIES,
        probcut: None,
        ..SearchConfig::standard()
    };
    let on = SearchConfig {
        probcut: Some(ProbCut {
            t,
            ..ProbCut::FITTED
        }),
        ..off
    };
    [("full", off), ("probcut", on)]
}

fn corpus() -> Vec<Board> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/corpus.txt");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let me = u64::from_str_radix(parts.next()?, 16).ok()?;
            let opp = u64::from_str_radix(parts.next()?, 16).ok()?;
            Some(Board { me, opp })
        })
        .filter(|board| legal_moves(board.me, board.opp) != 0)
        .filter(|board| empty_count(*board) > SPLIT_EMPTIES)
        .collect()
}

/// The position after `PLIES` plies of each of the first `GAMES` games.
fn positions() -> Vec<Board> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/tuning/speed_135_match.journal"
    );
    let text = std::fs::read_to_string(path).unwrap();
    text.lines()
        .filter_map(|line| line.split_once(" moves ").map(|(_, moves)| moves))
        .take(GAMES)
        .map(|moves| {
            let mut board = Board::START;
            for token in moves.split_whitespace().take(PLIES) {
                board = match token.as_bytes() {
                    b"pass" => pass(board),
                    &[file, rank] => play(board, (rank - b'1') * 8 + (file - b'a')),
                    _ => panic!("bad move {token}"),
                };
            }
            if legal_moves(board.me, board.opp) == 0 {
                board = pass(board);
            }
            board
        })
        .collect()
}

fn median(mut values: Vec<u32>) -> f64 {
    values.sort_unstable();
    let n = values.len();
    if n % 2 == 1 {
        f64::from(values[n / 2])
    } else {
        f64::from(values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

/// Options: `--t` for both commands, `--ms` and `--jobs` for `depth` only.
/// Each at most once, each with a value; anything else is an error.
struct Options {
    t: u32,
    ms: u64,
    jobs: usize,
}

fn options(command: &str, args: &[String]) -> Result<Options, String> {
    let allowed: &[&str] = if command == "depth" {
        &["--t", "--ms", "--jobs"]
    } else {
        &["--t"]
    };
    let mut seen = std::collections::HashMap::new();
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        if !allowed.contains(&flag.as_str()) {
            return Err(format!("unknown option {flag} for {command}"));
        }
        let value = rest.next().ok_or_else(|| format!("{flag} needs a value"))?;
        if seen.insert(flag.as_str(), value.as_str()).is_some() {
            return Err(format!("{flag} given twice"));
        }
    }
    let number = |flag: &str, default: Option<&str>| -> Result<u64, String> {
        let text = seen
            .get(flag)
            .copied()
            .or(default)
            .ok_or_else(|| format!("{flag} is required"))?;
        text.parse().map_err(|_| format!("invalid {flag} {text}"))
    };
    let t = u32::try_from(number("--t", None)?)
        .ok()
        .filter(|t| (50..=400).contains(t))
        .ok_or("--t must be 50..=400")?;
    let ms = number("--ms", Some("3000"))?;
    if ms == 0 {
        return Err("--ms must be positive".into());
    }
    let jobs = usize::try_from(number("--jobs", Some("5"))?)
        .ok()
        .filter(|jobs| (1..=64).contains(jobs))
        .ok_or("--jobs must be 1..=64")?;
    Ok(Options { t, ms, jobs })
}

fn run(args: &[String]) -> Result<(), String> {
    let command = args.first().map(String::as_str).unwrap_or("");
    if command != "nodes" && command != "depth" {
        return Err("usage: probcut_screen nodes --t T | depth --t T [--ms MS] [--jobs J]".into());
    }
    let Options { t, ms, jobs } = options(command, &args[1..])?;
    match command {
        "nodes" => {
            let boards = corpus();
            let mut totals = Vec::new();
            for (label, config) in configs(t) {
                let mut table = Table::new(MAX_ENTRIES);
                let mut nodes = 0;
                for &board in &boards {
                    table.clear();
                    let limits = SearchLimits::depth(MIDGAME_DEPTH);
                    nodes += search_tt(board, limits, &config, &mut table).nodes;
                }
                println!(
                    "{label} positions {} depth {MIDGAME_DEPTH} nodes {nodes}",
                    boards.len()
                );
                totals.push(nodes);
            }
            let ratio = totals[1] as f64 / totals[0] as f64;
            let verdict = if ratio <= 0.60 { "pass" } else { "fail" };
            println!("screen nodes t {t} ratio {ratio:.4} limit 0.6000 {verdict}");
        }
        _ => {
            let boards = positions();
            let mut medians = Vec::new();
            for (label, config) in configs(t) {
                let depths = Mutex::new(vec![0u32; boards.len()]);
                let next = AtomicUsize::new(0);
                std::thread::scope(|scope| {
                    for _ in 0..jobs {
                        scope.spawn(|| {
                            let mut table = Table::new(MAX_ENTRIES);
                            loop {
                                let index = next.fetch_add(1, Relaxed);
                                let Some(&board) = boards.get(index) else {
                                    break;
                                };
                                table.clear();
                                let limits = SearchLimits::new(64, ms, 0);
                                let result = search_tt(board, limits, &config, &mut table);
                                depths.lock().unwrap()[index] = result.depth;
                            }
                        });
                    }
                });
                let depths = depths.into_inner().unwrap();
                let list: Vec<String> = depths.iter().map(u32::to_string).collect();
                println!(
                    "{label} positions {} ms {ms} depths {}",
                    boards.len(),
                    list.join(",")
                );
                medians.push(median(depths));
            }
            let verdict = if medians[1] >= medians[0] + 1.0 {
                "pass"
            } else {
                "fail"
            };
            println!(
                "screen depth t {t} median_full {:.1} median_probcut {:.1} {verdict}",
                medians[0], medians[1]
            );
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(message) = run(&args) {
        eprintln!("{message}");
        std::process::exit(2);
    }
}
