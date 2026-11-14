//! ProbCut v2 screen (docs/design/probcut2.md § Screening and tuning `t`,
//! the registered screen), for one candidate `t` in hundredths of σ with the
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
//! - `diag` (the strength gate's separate, non-gated diagnostic): the
//!   first 50 openings of `bench/openings_probcut3.txt`, each continued to 20
//!   plies by the full-width standard engine at fixed depth 6 (cleared table
//!   per move; a game that ends first is reported and left out); for each, the
//!   depth completed in `--ms` with and without ProbCut (standard solve
//!   threshold, fresh table), and ProbCut's tries and cuts in a cold
//!   fixed-depth search at the depth ProbCut completed.
//!
//! usage: probcut_screen nodes --t T
//!        probcut_screen depth --t T [--ms 3000] [--jobs 5]
//!        probcut_screen diag --t T [--ms 3000] [--jobs 5]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use othello::board::{Board, empty_count, legal_moves, pass, play};
use othello::score::INF;
use othello::search::{Ctx, ProbCut, SearchConfig, SearchLimits, negamax, search_tt};
use othello::tt::{MAX_ENTRIES, Table};

const SPLIT_EMPTIES: u32 = 14;
const MIDGAME_DEPTH: u32 = 10;
const GAMES: usize = 50;
const PLIES: usize = 20;

fn configs(t: u32, threshold: u32) -> [(&'static str, SearchConfig); 2] {
    let off = SearchConfig {
        endgame_threshold: threshold,
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
    let allowed: &[&str] = if command != "nodes" {
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
    if !["nodes", "depth", "diag"].contains(&command) {
        return Err(
            "usage: probcut_screen nodes --t T | depth|diag --t T [--ms MS] [--jobs J]".into(),
        );
    }
    let Options { t, ms, jobs } = options(command, &args[1..])?;
    match command {
        "nodes" => {
            let boards = corpus();
            let mut totals = Vec::new();
            for (label, config) in configs(t, SPLIT_EMPTIES) {
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
        "depth" => {
            let boards = positions();
            let mut medians = Vec::new();
            for (label, config) in configs(t, SPLIT_EMPTIES) {
                let depths = depths(&boards, &config, ms, jobs);
                println!(
                    "{label} positions {} ms {ms} depths {}",
                    boards.len(),
                    list(&depths)
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
        _ => diag(t, ms, jobs)?,
    }
    Ok(())
}

fn list(values: &[u32]) -> String {
    values
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// The depth each board's timed search completes, `jobs` boards at a time,
/// each with a cleared table.
fn depths(boards: &[Board], config: &SearchConfig, ms: u64, jobs: usize) -> Vec<u32> {
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
                    let result = search_tt(board, limits, config, &mut table);
                    depths.lock().unwrap()[index] = result.depth;
                }
            });
        }
    });
    depths.into_inner().unwrap()
}

const GATE_OPENINGS: usize = 50;
const GATE_DEPTH: u32 = 6;

/// The first gate openings continued to `PLIES` plies (passes not counted) by
/// the full-width standard engine at `GATE_DEPTH`; `None` for a game that
/// ends first.
fn gate_positions() -> Result<Vec<Option<Board>>, String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bench/openings_probcut3.txt"
    );
    let config = SearchConfig {
        probcut: None,
        ..SearchConfig::standard()
    };
    let mut table = Table::new(MAX_ENTRIES);
    let openings = othello_tools::positions::load(path)?;
    let continued = openings.iter().take(GATE_OPENINGS).map(|opening| {
        let mut board = opening
            .squares
            .iter()
            .fold(Board::START, |board, &sq| play(board, sq));
        let mut plies = opening.squares.len();
        while plies < PLIES {
            if legal_moves(board.me, board.opp) == 0 {
                if legal_moves(board.opp, board.me) == 0 {
                    return None;
                }
                board = pass(board);
                continue;
            }
            table.clear();
            let best = search_tt(board, SearchLimits::depth(GATE_DEPTH), &config, &mut table).best;
            board = play(board, best);
            plies += 1;
        }
        if legal_moves(board.me, board.opp) == 0 {
            board = pass(board);
        }
        Some(board)
    });
    Ok(continued.collect())
}

fn diag(t: u32, ms: u64, jobs: usize) -> Result<(), String> {
    let all = gate_positions()?;
    let ended: Vec<usize> = (0..all.len()).filter(|&i| all[i].is_none()).collect();
    let boards: Vec<Board> = all.into_iter().flatten().collect();
    println!(
        "diag gate openings {GATE_OPENINGS} plies {PLIES} continuation depth {GATE_DEPTH} positions {} ended {ended:?} ms {ms} jobs {jobs}",
        boards.len()
    );
    let [(_, off), (_, on)] = configs(t, SearchConfig::standard().endgame_threshold);
    let full = depths(&boards, &off, ms, jobs);
    let selective = depths(&boards, &on, ms, jobs);
    println!("full depths {}", list(&full));
    println!("probcut depths {}", list(&selective));
    let mut table = Table::new(MAX_ENTRIES);
    let (mut tries, mut cuts) = (0, [0, 0]);
    println!("cold fixed-depth ProbCut search at the ProbCut depth: position depth tries high low");
    for (index, (&board, &depth)) in boards.iter().zip(&selective).enumerate() {
        table.clear();
        let mut ctx = Ctx::new(&on);
        ctx.set_table(&mut table);
        negamax(board, depth, -INF, INF, &mut ctx);
        println!(
            "  {index} {depth} {} {} {}",
            ctx.probcut_tries, ctx.probcut_cuts[0], ctx.probcut_cuts[1]
        );
        tries += ctx.probcut_tries;
        cuts = [cuts[0] + ctx.probcut_cuts[0], cuts[1] + ctx.probcut_cuts[1]];
    }
    println!(
        "diag t {t} median_full {:.1} median_probcut {:.1} tries {tries} cuts_high {} cuts_low {}",
        median(full),
        median(selective),
        cuts[0],
        cuts[1]
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(message) = run(&args) {
        eprintln!("{message}");
        std::process::exit(2);
    }
}
