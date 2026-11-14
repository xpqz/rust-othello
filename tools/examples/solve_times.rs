//! Time to solve, per position, at several empties counts, on the positions of
//! a recorded match: from each game, the first position with exactly N empties
//! and a legal move, solved with the threshold set to N. Cold: table cleared
//! before the solve. Previous-move warmed: the same side's previous move is
//! searched first on the same table, as a match worker would, and the solve
//! follows without clearing. One journal line per position, so the job resumes
//! by re-running the same command; the summary is computed from the journal.
//! usage: solve_times --out FILE [--empties 16,18,20] [--budget 30000]
//!        [--previous 3000] [--progress 30]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use othello::board::{Board, empty_count, legal_moves, pass, play};
use othello::search::{SearchConfig, SearchLimits, search_tt};
use othello::tt::{MAX_ENTRIES, Table};
use othello_tools::journal::Journal;
use othello_tools::provenance::{file_hash, git_commit};

const SOURCE: &str = "docs/tuning/speed_135_match.journal";

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

/// The position to solve and the same side's previous position, if any.
fn unit(positions: &[(Board, bool)], empties: u32) -> Option<(Board, Option<Board>)> {
    let movable = |board: Board| legal_moves(board.me, board.opp) != 0;
    let at = positions
        .iter()
        .position(|&(board, _)| empty_count(board) == empties && movable(board))?;
    let (board, black) = positions[at];
    let earlier = positions[..at]
        .iter()
        .rev()
        .find(|&&(prior, colour)| colour == black && movable(prior));
    Some((board, earlier.map(|&(prior, _)| prior)))
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let mut words = line.split_whitespace();
    while let Some(word) = words.next() {
        if word == name {
            return words.next();
        }
    }
    None
}

fn summary(label: &str, empties: &str, mut times: Vec<f64>, unsolved: usize) {
    times.sort_by(|a, b| a.total_cmp(b));
    let rank = |fraction: f64| {
        let index = (times.len() as f64 * fraction).ceil() as usize;
        times[index.clamp(1, times.len()) - 1]
    };
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    println!(
        "{label} empties {empties} positions {} mean_ms {mean:.1} p95_ms {:.1} p99_ms {:.1} max_ms {:.1} unsolved {unsolved}",
        times.len(),
        rank(0.95),
        rank(0.99),
        times[times.len() - 1]
    );
}

fn run(args: &[String]) -> Result<(), String> {
    let value = |flag: &str, default: &str| {
        let at = args.iter().position(|arg| arg == flag);
        at.and_then(|at| args.get(at + 1))
            .map_or(default.to_string(), String::clone)
    };
    let number = |flag: &str, default: &str| {
        let text = value(flag, default);
        text.parse::<u64>().map_err(|_| format!("invalid {flag}"))
    };
    let out = value("--out", "");
    if out.is_empty() {
        return Err("usage: solve_times --out FILE [--empties 16,18,20] [--budget MS] [--previous MS] [--progress S]".into());
    }
    let list = value("--empties", "16,18,20");
    let counts: Vec<u32> = list
        .split(',')
        .map(|n| n.parse().map_err(|_| "invalid --empties".to_string()))
        .collect::<Result<_, _>>()?;
    if (1..counts.len()).any(|index| counts[..index].contains(&counts[index])) {
        return Err("each --empties count may be given once".into());
    }
    let (budget, previous) = (number("--budget", "30000")?, number("--previous", "3000")?);
    let progress = number("--progress", "30")?.max(1);
    let source = format!("{}/../{SOURCE}", env!("CARGO_MANIFEST_DIR"));
    // The executable's own hash: timings from a different build (profile,
    // compiler, code or embedded weights) must not join this journal.
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let header = format!(
        "header v1 solve_times empties {list} budget_ms {budget} previous_ms {previous} source {SOURCE} hash {:016x} commit {} binary {:016x}",
        file_hash(&source)?,
        git_commit(),
        file_hash(&exe.to_string_lossy())?
    );
    let (mut journal, done) = Journal::open(Path::new(&out), &header)?;
    let text = std::fs::read_to_string(&source).map_err(|error| format!("{source}: {error}"))?;
    let games = games(&text);
    let mut units = Vec::new();
    for &empties in &counts {
        for (game, positions) in games.iter().enumerate() {
            if let Some((board, earlier)) = unit(positions, empties) {
                units.push((format!("{empties}:{game}"), empties, board, earlier));
            }
        }
    }
    let total = units.len();
    let finished = AtomicUsize::new(done.len());
    let stop = AtomicBool::new(false);
    let start = Instant::now();
    std::thread::scope(|scope| -> Result<(), String> {
        scope.spawn(|| {
            let mut last = Instant::now();
            while !stop.load(Relaxed) {
                std::thread::sleep(Duration::from_millis(200));
                if last.elapsed() >= Duration::from_secs(progress) {
                    last = Instant::now();
                    let count = finished.load(Relaxed);
                    let fresh = (count - done.len()).max(1) as f64;
                    let eta = start.elapsed().as_secs_f64() / fresh * (total - count) as f64;
                    eprintln!(
                        "progress {count}/{total} positions, elapsed {:.0}s eta {eta:.0}s",
                        start.elapsed().as_secs_f64()
                    );
                }
            }
        });
        let mut table = Table::new(MAX_ENTRIES);
        let result = (|| {
            for (id, empties, board, earlier) in units.iter().filter(|unit| !done.contains(&unit.0))
            {
                let config = SearchConfig {
                    endgame_threshold: *empties,
                    ..SearchConfig::standard()
                };
                let solve = |table: &mut Table| {
                    let begin = Instant::now();
                    let result =
                        search_tt(*board, SearchLimits::new(64, budget, 0), &config, table);
                    (begin.elapsed().as_secs_f64() * 1000.0, result)
                };
                table.clear();
                let (cold_ms, cold) = solve(&mut table);
                table.clear();
                if let Some(earlier) = earlier {
                    search_tt(
                        *earlier,
                        SearchLimits::new(64, previous, 0),
                        &config,
                        &mut table,
                    );
                }
                let (warm_ms, warm) = solve(&mut table);
                if cold.solved && warm.solved && cold.score != warm.score {
                    return Err(format!("{id}: scores differ {} {}", cold.score, warm.score));
                }
                journal.append(
                    id,
                    &format!(
                        "board {:016x}:{:016x} cold_ms {cold_ms:.1} cold_solved {} cold_score {} warm_ms {warm_ms:.1} warm_solved {} warm_score {}",
                        board.me, board.opp, cold.solved as u8, cold.score, warm.solved as u8, warm.score
                    ),
                )?;
                finished.fetch_add(1, Relaxed);
            }
            Ok(())
        })();
        stop.store(true, Relaxed);
        result
    })?;
    let mut groups: BTreeMap<&str, [(Vec<f64>, usize); 2]> = BTreeMap::new();
    for record in journal.records() {
        let (empties, _) = record.id.split_once(':').ok_or("invalid journal id")?;
        let group = groups.entry(empties).or_default();
        for (slot, (time, solved)) in [("cold_ms", "cold_solved"), ("warm_ms", "warm_solved")]
            .iter()
            .enumerate()
        {
            let ms = field(&record.line, time).and_then(|ms| ms.parse().ok());
            group[slot].0.push(ms.ok_or("invalid journal line")?);
            group[slot].1 += usize::from(field(&record.line, solved) != Some("1"));
        }
    }
    println!("solve_times {header}");
    for (empties, [cold, warm]) in groups {
        summary("cold", empties, cold.0, cold.1);
        summary("previous_move_warmed", empties, warm.0, warm.1);
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
