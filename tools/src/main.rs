use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::{Command, exit};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use othello::rng::Rng;
use othello::search::{MAX_CLOCK_MS, NO_DEADLINE, SearchConfig};
use othello_tools::agents::{build, clock, config, effective, parse_name};
use othello_tools::harness::{GameLog, clocked_points, play_clocked};
use othello_tools::journal::{Journal, Record, mismatch};
use othello_tools::positions::{self, Opening};
use othello_tools::provenance::{file_hash, git_commit};
use othello_tools::stats::{Interval, RESAMPLES, report_with};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((command, rest)) if command == "match" => match_command(rest),
        Some((command, rest)) if command == "report" => report_command(rest),
        _ => Err("usage: othello-tools match|report [options]".into()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        exit(2);
    }
}

fn describe(name: &str, nodes: u64) -> String {
    let (kind, depth, time) = parse_name(name).unwrap();
    if kind == "random" {
        return "random".into();
    }
    if kind == "greedy" {
        return "greedy (disc-greedy)".into();
    }
    let (depth, time) = effective(depth, time, nodes).unwrap();
    let time = if time == NO_DEADLINE {
        "none".into()
    } else {
        time.to_string()
    };
    let eval = match kind {
        "phase2" => "eval_phase2",
        "classic" => "eval_classic",
        _ => "pattern::evaluate",
    };
    let threshold = match kind {
        "phase2" => 12,
        "classic" => 14,
        _ => SearchConfig::standard().endgame_threshold,
    };
    format!("{name} ({eval}, depth {depth}, time {time}, nodes {nodes}, threshold {threshold})")
}

fn stream(seed: u64, pair: usize, game: u64, side: u64) -> u64 {
    Rng::new(Rng::new(seed).next_u64() ^ ((pair as u64) << 2 | (game << 1) | side)).next_u64()
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let mut parts = line.split_whitespace();
    while let Some(part) = parts.next() {
        if part == key {
            return parts.next();
        }
    }
    None
}

fn get<T: FromStr>(line: &str, key: &str) -> Result<T, String> {
    field(line, key)
        .ok_or_else(|| format!("missing {key}"))?
        .parse()
        .map_err(|_| format!("invalid {key}"))
}

fn control(a: &str, b: &str, nodes: u64) -> Result<&'static str, String> {
    let timed = |name: &str| parse_name(name).map(|(_, _, time)| time.is_some());
    if clock(a)?.is_some() || clock(b)?.is_some() {
        Ok("clock")
    } else if nodes > 0 {
        Ok("nodes")
    } else if timed(a)? || timed(b)? {
        Ok("time")
    } else {
        Ok("depth")
    }
}

/// The load as journalled, to three decimals: every decision uses this value.
fn rounded(load: f64) -> f64 {
    (load * 1000.0).round() / 1000.0
}

fn load_average() -> f64 {
    // Tests fix the measured load here.
    if let Ok(load) = std::env::var("OTHELLO_TOOLS_LOAD") {
        return load.parse().expect("OTHELLO_TOOLS_LOAD is a number");
    }
    if let Ok(text) = fs::read_to_string("/proc/loadavg")
        && let Some(value) = text.split_whitespace().next().and_then(|v| v.parse().ok())
    {
        return value;
    }
    let output = Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .ok();
    output
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .find_map(|token| token.trim_matches(['{', '}']).parse().ok())
        })
        .unwrap_or(0.0)
}

#[derive(Clone, Copy, Debug, Default)]
struct Saved {
    points: f64,
    load: f64,
    /// Present when the record carries clock data (real-time controls).
    clock: Option<ClockData>,
}

/// Charged ms for black and white, and the side that flagged.
type ClockData = ([u64; 2], Option<usize>);

/// A record's clock data: all three fields, valid, or none of them.
fn clock_data(id: &str, tail: &str) -> Result<Option<ClockData>, String> {
    let keys = ["black_ms", "white_ms", "flag"];
    match keys.iter().filter(|key| field(tail, key).is_some()).count() {
        0 => return Ok(None),
        3 => {}
        _ => return Err(format!("record {id}: partial clock data")),
    }
    let bad = |_| format!("record {id}: invalid clock data");
    let ms = [
        get(tail, "black_ms").map_err(bad)?,
        get(tail, "white_ms").map_err(bad)?,
    ];
    let flag = match field(tail, "flag") {
        Some("none") => None,
        Some("black") => Some(0),
        Some("white") => Some(1),
        _ => return Err(bad(String::new())),
    };
    Ok(Some((ms, flag)))
}

fn parse_saved(id: &str, line: &str) -> Result<(usize, u64, Saved), String> {
    let (pair, game) = id.split_once(':').ok_or("invalid journal id")?;
    let (_, tail) = line.rsplit_once(" points ").ok_or("missing points")?;
    let parts: Vec<&str> = tail.split_whitespace().collect();
    Ok((
        pair.parse().map_err(|_| "invalid pair")?,
        game.parse().map_err(|_| "invalid game")?,
        Saved {
            points: parts
                .first()
                .ok_or("missing points")?
                .parse()
                .map_err(|_| "invalid points")?,
            load: parts
                .get(2)
                .ok_or("missing load")?
                .parse()
                .map_err(|_| "invalid load")?,
            clock: clock_data(id, tail)?,
        },
    ))
}

fn void(saved: Saved, control: &str, max_load: f64) -> bool {
    matches!(control, "time" | "clock") && saved.load > max_load
}

struct Outcome {
    pair: usize,
    points: [f64; 2],
    logs: Vec<(u64, String)>,
    load: f64,
}

#[derive(Clone)]
struct Run {
    a: String,
    b: String,
    pairs: usize,
    seed: u64,
    nodes: u64,
    ordering: bool,
    jobs: usize,
    max_load: f64,
    openings: String,
    out: Option<String>,
    first: usize,
    past: usize,
    /// `OTHELLO_TOOLS_ELAPSED_MS`: a fixed charge per move in place of the
    /// measured time, part of the journal's identity.
    elapsed: Option<u64>,
}

impl Run {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut openings = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../bench/openings_validation.txt"
        )
        .to_string();
        let (mut a, mut b, mut pairs, mut seed, mut nodes) = (None, None, 100usize, 1u64, 0u64);
        let (mut ordering, mut jobs, mut max_load, mut out, mut range) =
            (true, 1usize, None, None, None);
        let mut index = 0;
        while index < args.len() {
            if args[index] == "--no-ordering" {
                ordering = false;
                index += 1;
                continue;
            }
            let value = args.get(index + 1).ok_or("missing option value")?;
            match args[index].as_str() {
                "--a" => a = Some(value.clone()),
                "--b" => b = Some(value.clone()),
                "--pairs" => pairs = value.parse().map_err(|_| "invalid pairs")?,
                "--seed" => seed = value.parse().map_err(|_| "invalid seed")?,
                "--openings" => openings = value.clone(),
                "--nodes" => nodes = value.parse().map_err(|_| "invalid nodes")?,
                "--jobs" => jobs = value.parse().map_err(|_| "invalid jobs")?,
                "--max-load" => max_load = Some(value.parse().map_err(|_| "invalid max-load")?),
                "--out" => out = Some(value.clone()),
                "--pair-range" => range = Some(parse_range(value)?),
                flag => return Err(format!("unknown option: {flag}")),
            }
            index += 2;
        }
        let explicit = range.is_some();
        let (first, past) = range.unwrap_or((0, pairs));
        if first > past || (explicit && first == past) || past > pairs || jobs == 0 {
            return Err("invalid range or jobs".into());
        }
        let a = a.ok_or("missing --a")?;
        let b = b.ok_or("missing --b")?;
        for name in [&a, &b] {
            if !matches!(parse_name(name)?.0, "random" | "greedy") {
                config(name, nodes, ordering)?;
            }
        }
        // Bounded like a clock, so charged totals fit the journal and report.
        let elapsed = match std::env::var("OTHELLO_TOOLS_ELAPSED_MS") {
            Ok(text) => Some(
                text.parse()
                    .ok()
                    .filter(|ms| *ms <= MAX_CLOCK_MS)
                    .ok_or("invalid OTHELLO_TOOLS_ELAPSED_MS")?,
            ),
            Err(_) => None,
        };
        let max_load = match max_load {
            Some(value) => value,
            None if matches!(control(&a, &b, nodes)?, "time" | "clock") => jobs as f64 + 2.0,
            None => jobs as f64,
        };
        let max_load = rounded(max_load);
        if max_load <= 0.0 {
            return Err("max-load must be positive".into());
        }
        Ok(Self {
            a,
            b,
            pairs,
            seed,
            nodes,
            ordering,
            jobs,
            max_load,
            openings,
            out,
            first,
            past,
            elapsed,
        })
    }

    fn header(&self) -> Result<String, String> {
        let elapsed = self
            .elapsed
            .map_or(String::new(), |ms| format!(" elapsed {ms}"));
        Ok(format!(
            "header v1 a {} b {} pairs {} seed {} nodes {} ordering {} control {} max_load {:.3} openings {} hash {:016x} engine {}",
            self.a,
            self.b,
            self.pairs,
            self.seed,
            self.nodes,
            self.ordering,
            control(&self.a, &self.b, self.nodes)?,
            self.max_load,
            self.openings,
            file_hash(&self.openings)?,
            git_commit()
        ) + &elapsed)
    }

    fn play(
        &self,
        opening: &Opening,
        pair: usize,
        journal: Option<&Mutex<Journal>>,
    ) -> Result<Outcome, String> {
        let mut points_a = [0.0; 2];
        let (mut logs, mut clock_fields) = (Vec::new(), Vec::new());
        for game in 0..2u64 {
            let seeds = (
                stream(self.seed, pair, game, 0),
                stream(self.seed, pair, game, 1),
            );
            let (black, white) = if game == 0 {
                (&self.a, &self.b)
            } else {
                (&self.b, &self.a)
            };
            let mut black_agent = build(black, seeds.0, self.nodes, self.ordering)?;
            let mut white_agent = build(white, seeds.1, self.nodes, self.ordering)?;
            let clocks = [clock(black)?, clock(white)?].map(|c| c.map(Duration::from_millis));
            let fixed = self.elapsed.map(Duration::from_millis);
            let mut elapsed = |start: Instant| fixed.unwrap_or_else(|| start.elapsed());
            let game_result = play_clocked(
                &opening.squares,
                &mut *black_agent,
                &mut *white_agent,
                clocks,
                &mut elapsed,
            )?;
            let result = game_result.result;
            let black_points = clocked_points(&game_result);
            points_a[game as usize] = if game == 0 {
                black_points
            } else {
                1.0 - black_points
            };
            let flag = game_result
                .flag
                .map_or("none", |side| ["black", "white"][side]);
            let [black_ms, white_ms] = game_result.used.map(|used| used.as_millis());
            let line = GameLog {
                run: self.seed,
                pair,
                game,
                agent_seeds: seeds,
                key: opening.key,
                opening: opening.line(),
                black: black.to_string(),
                white: white.to_string(),
                score: result.score,
            }
            .line();
            logs.push((game, line));
            // Real-time controls only: depth and node runs stay reproducible.
            let timed = matches!(control(&self.a, &self.b, self.nodes)?, "time" | "clock");
            clock_fields.push(if timed {
                format!(" black_ms {black_ms} white_ms {white_ms} flag {flag}")
            } else {
                String::new()
            });
        }
        let load = rounded(load_average());
        if let Some(journal) = journal {
            let mut journal = journal.lock().unwrap();
            for (game, line) in &logs {
                let payload = format!(
                    "{line} points {:.4} load {load:.3} jobs {}{}",
                    points_a[*game as usize], self.jobs, clock_fields[*game as usize]
                );
                journal.append(&format!("{pair}:{game}"), &payload)?;
            }
        }
        Ok(Outcome {
            pair,
            points: points_a,
            logs,
            load,
        })
    }

    fn run(
        &self,
        queued: &[usize],
        openings: &[Opening],
        journal: Option<&Arc<Mutex<Journal>>>,
    ) -> Result<Vec<Outcome>, String> {
        if queued.is_empty() {
            return Ok(Vec::new());
        }
        let next = AtomicUsize::new(0);
        let (sender, receiver) = mpsc::channel();
        let start = Instant::now();
        let (mut outcomes, mut errors) = (Vec::new(), Vec::new());
        eprintln!("pairs 0/{} queued, {} jobs", queued.len(), self.jobs);
        thread::scope(|scope| {
            for _ in 0..self.jobs.min(queued.len()) {
                let sender = sender.clone();
                let journal = journal.cloned();
                let next = &next;
                scope.spawn(move || {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&pair) = queued.get(index) else {
                            break;
                        };
                        if sender
                            .send(self.play(&openings[pair], pair, journal.as_deref()))
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            }
            drop(sender);
            let (mut last, mut done) = (Instant::now(), 0);
            loop {
                match receiver.recv_timeout(Duration::from_secs(1)) {
                    Ok(Ok(outcome)) => {
                        outcomes.push(outcome);
                        done += 1;
                    }
                    Ok(Err(error)) => {
                        errors.push(error);
                        done += 1;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if last.elapsed() >= Duration::from_secs(60) {
                    eprintln!(
                        "pairs {done}/{} elapsed {:.1}s",
                        queued.len(),
                        start.elapsed().as_secs_f64()
                    );
                    last = Instant::now();
                }
            }
        });
        errors.into_iter().next().map_or(Ok(outcomes), Err)
    }
}

fn print_summary(summary: &othello_tools::stats::Report, seed: u64, nodes: u64, ordering: bool) {
    let interval = match summary.interval {
        Interval::Insufficient => "insufficient".into(),
        Interval::Uninformative { low, high, .. } => format!("[{low:.4}, {high:.4}] uninformative"),
        Interval::Bootstrap { low, high, .. } => format!("[{low:.4}, {high:.4}] bootstrap"),
    };
    let elo = summary
        .interval
        .informative()
        .then_some(summary.elo)
        .flatten()
        .map_or("none".into(), |(low, high)| {
            format!("[{low:.2}, {high:.2}]")
        });
    println!(
        "pairs {} games {} W {} D {} L {} mean {:.4} interval {interval} elo {elo}",
        summary.pairs, summary.games, summary.wins, summary.draws, summary.losses, summary.mean
    );
    println!("seed {seed} resamples {RESAMPLES} evaluator-only nodes {nodes} ordering {ordering}");
}

fn pair_points(
    saved: &BTreeMap<(usize, u64), Saved>,
    pair: usize,
    ctrl: &str,
    max_load: f64,
) -> Option<(f64, f64)> {
    let left = saved.get(&(pair, 0))?;
    let right = saved.get(&(pair, 1))?;
    (!void(*left, ctrl, max_load) && !void(*right, ctrl, max_load))
        .then_some((left.points, right.points))
}

fn match_command(args: &[String]) -> Result<(), String> {
    let run = Run::parse(args)?;
    let openings = positions::load(&run.openings)?;
    if openings.is_empty() {
        return Err("no openings".into());
    }
    if run.pairs > openings.len() {
        return Err(format!(
            "pairs {} exceeds {} openings",
            run.pairs,
            openings.len()
        ));
    }
    let ctrl = control(&run.a, &run.b, run.nodes)?;
    let mut journal = None;
    let mut saved = BTreeMap::new();
    if let Some(path) = &run.out {
        let (opened, _) = Journal::open(Path::new(path), &run.header()?)?;
        for record in opened.records().to_vec() {
            let (pair, game, record) = parse_saved(&record.id, &record.line)?;
            saved.insert((pair, game), record);
        }
        journal = Some(Arc::new(Mutex::new(opened)));
    }
    // Saved and fresh pairs alike: a pair above the load limit is discarded
    // and played again, up to ten passes.
    let mut outcomes = Vec::new();
    for pass in 0.. {
        // Missing and half-played pairs count as void too: nothing valid to keep.
        let voided: Vec<usize> = (run.first..run.past)
            .filter(|&pair| pair_points(&saved, pair, ctrl, run.max_load).is_none())
            .collect();
        for pair in &voided {
            if let Some(over) = saved
                .get(&(*pair, 0))
                .filter(|s| void(**s, ctrl, run.max_load))
            {
                eprintln!("void pair {pair} load {:.3}", over.load);
            }
        }
        saved.retain(|(pair, _), _| !voided.contains(pair));
        let discard: BTreeSet<String> = (voided.iter())
            .flat_map(|pair| [format!("{pair}:0"), format!("{pair}:1")])
            .collect();
        if let Some(journal) = &journal {
            journal.lock().unwrap().discard(&discard)?;
        }
        outcomes.retain(|outcome: &Outcome| !voided.contains(&outcome.pair));
        let queued: Vec<usize> = (run.first..run.past)
            .filter(|&pair| !saved.contains_key(&(pair, 0)))
            .collect();
        if queued.is_empty() {
            break;
        }
        if pass == 10 {
            return Err(format!("{} pairs void after ten passes", queued.len()));
        }
        for outcome in run.run(&queued, &openings, journal.as_ref())? {
            for (game, &points) in (0..).zip(&outcome.points) {
                let load = outcome.load;
                let fresh = Saved {
                    points,
                    load,
                    ..Saved::default()
                };
                saved.insert((outcome.pair, game), fresh);
            }
            outcomes.push(outcome);
        }
    }
    let game_points: Vec<(f64, f64)> = (run.first..run.past)
        .map(|pair| Ok((saved[&(pair, 0)].points, saved[&(pair, 1)].points)))
        .collect::<Result<_, String>>()?;
    print_summary(
        &report_with(&game_points, RESAMPLES, run.seed),
        run.seed,
        run.nodes,
        run.ordering,
    );
    println!("a {}", describe(&run.a, run.nodes));
    println!("b {}", describe(&run.b, run.nodes));
    let mut logs: Vec<(usize, u64, String)> = outcomes
        .iter()
        .flat_map(|outcome| {
            outcome
                .logs
                .iter()
                .map(|(game, line)| (outcome.pair, *game, line.clone()))
        })
        .collect();
    logs.sort();
    logs.into_iter().for_each(|(_, _, line)| println!("{line}"));
    Ok(())
}

fn parse_range(text: &str) -> Result<(usize, usize), String> {
    let (first, past) = text.split_once("..").ok_or("invalid pair range")?;
    Ok((
        first.parse().map_err(|_| "invalid pair range")?,
        past.parse().map_err(|_| "invalid pair range")?,
    ))
}

fn report_command(args: &[String]) -> Result<(), String> {
    let partial = args.iter().any(|arg| arg == "--partial");
    let paths: Vec<&String> = args.iter().filter(|arg| *arg != "--partial").collect();
    if paths.is_empty() {
        return Err("usage: othello-tools report [--partial] <journal>...".into());
    }
    let mut header: Option<String> = None;
    let mut records: BTreeMap<(usize, u64), (usize, Saved)> = BTreeMap::new();
    for (source, path) in paths.iter().enumerate() {
        let (found, journal) = othello_tools::journal::read(Path::new(path))?;
        if found.is_empty() {
            return Err(format!("{path}: empty journal"));
        }
        if let Some(expected) = &header {
            if expected != &found {
                return Err(mismatch(expected, &found));
            }
        } else {
            header = Some(found);
        }
        for Record { id, line } in journal {
            let (pair, game, saved) = parse_saved(&id, &line)?;
            if records
                .get(&(pair, game))
                .is_some_and(|(other, _)| *other != source)
            {
                return Err(format!("duplicate pair {pair} across journals"));
            }
            records.insert((pair, game), (source, saved));
        }
    }
    let header = header.unwrap();
    let pairs = get::<usize>(&header, "pairs")?;
    let seed = get::<u64>(&header, "seed")?;
    let max_load = get::<f64>(&header, "max_load")?;
    let ctrl = field(&header, "control").ok_or("missing control")?;
    let mut game_points = Vec::new();
    let mut missing = Vec::new();
    // Per agent (a, b): charged ms and flags; a plays black in game 0.
    // `complete` until a used record lacks clock data.
    let (mut ms, mut flags, mut complete) = ([0u128; 2], [0usize; 2], true);
    for pair in 0..pairs {
        let valid = records.get(&(pair, 0)).zip(records.get(&(pair, 1))).filter(
            |((_, left), (_, right))| !void(*left, ctrl, max_load) && !void(*right, ctrl, max_load),
        );
        let Some(((_, left), (_, right))) = valid else {
            missing.push(pair);
            continue;
        };
        game_points.push((left.points, right.points));
        let timed = matches!(ctrl, "time" | "clock");
        for (game, saved) in [left, right].into_iter().enumerate().filter(|_| timed) {
            let Some((charged, flag)) = saved.clock else {
                if ctrl == "clock" {
                    return Err(format!("record {pair}:{game}: missing clock data"));
                }
                complete = false;
                continue;
            };
            for (side, charged) in charged.into_iter().enumerate() {
                ms[side ^ game] += u128::from(charged);
            }
            if let Some(side) = flag {
                flags[side ^ game] += 1;
            }
        }
    }
    if !missing.is_empty() {
        if !partial {
            return Err(format!("missing pairs: {missing:?}"));
        }
        println!("PARTIAL missing pairs: {missing:?}");
    }
    print_summary(
        &report_with(&game_points, RESAMPLES, seed),
        seed,
        get(&header, "nodes")?,
        true,
    );
    let games = (2 * game_points.len()).max(1) as u128;
    if !complete {
        println!("time unavailable");
    } else if matches!(ctrl, "time" | "clock") {
        println!(
            "time a {} b {} flags a {} b {}",
            ms[0] / games,
            ms[1] / games,
            flags[0],
            flags[1]
        );
    }
    println!("{header}");
    Ok(())
}
