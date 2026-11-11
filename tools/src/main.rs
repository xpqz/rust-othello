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
use othello::search::{NO_DEADLINE, SearchConfig};
use othello_tools::agents::{build, effective, parse_name};
use othello_tools::harness::{GameLog, play_game, points};
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
    let eval = if kind == "phase2" {
        "eval_phase2"
    } else {
        "evaluate"
    };
    let threshold = if kind == "phase2" {
        12
    } else {
        SearchConfig::standard().endgame_threshold
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
    if nodes > 0 {
        Ok("nodes")
    } else if timed(a)? || timed(b)? {
        Ok("time")
    } else {
        Ok("depth")
    }
}

fn load_average() -> f64 {
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

#[derive(Clone, Copy, Debug)]
struct Saved {
    points: f64,
    load: f64,
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
        },
    ))
}

fn void(saved: Saved, control: &str, max_load: f64) -> bool {
    control == "time" && saved.load > max_load
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
        let max_load = match max_load {
            Some(value) => value,
            None if control(&a, &b, nodes)? == "time" => jobs as f64 + 2.0,
            None => jobs as f64,
        };
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
        })
    }

    fn header(&self) -> Result<String, String> {
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
        ))
    }

    fn play(
        &self,
        opening: &Opening,
        pair: usize,
        journal: Option<&Mutex<Journal>>,
    ) -> Result<Outcome, String> {
        let mut points_a = [0.0; 2];
        let mut logs = Vec::new();
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
            let result = play_game(&opening.squares, &mut *black_agent, &mut *white_agent)?;
            points_a[game as usize] = if game == 0 {
                points(result.score)
            } else {
                1.0 - points(result.score)
            };
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
        }
        let load = load_average();
        if let Some(journal) = journal {
            let mut journal = journal.lock().unwrap();
            for (game, line) in &logs {
                let payload = format!(
                    "{line} points {:.4} load {load:.3} jobs {}",
                    points_a[*game as usize], self.jobs
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
    let mut journal = None;
    let mut saved = BTreeMap::new();
    if let Some(path) = &run.out {
        let (mut opened, _) = Journal::open(Path::new(path), &run.header()?)?;
        for record in opened.records().to_vec() {
            let (pair, game, record) = parse_saved(&record.id, &record.line)?;
            saved.insert((pair, game), record);
        }
        let ctrl = control(&run.a, &run.b, run.nodes)?;
        let mut discard = BTreeSet::new();
        for pair in run.first..run.past {
            if pair_points(&saved, pair, ctrl, run.max_load).is_none() {
                for game in 0..2 {
                    if saved.remove(&(pair, game)).is_some() {
                        discard.insert(format!("{pair}:{game}"));
                    }
                }
            }
        }
        opened.discard(&discard)?;
        journal = Some(Arc::new(Mutex::new(opened)));
    }
    let queued: Vec<usize> = (run.first..run.past)
        .filter(|&pair| !saved.contains_key(&(pair, 0)))
        .collect();
    let outcomes = run.run(&queued, &openings, journal.as_ref())?;
    for outcome in &outcomes {
        saved.insert(
            (outcome.pair, 0),
            Saved {
                points: outcome.points[0],
                load: outcome.load,
            },
        );
        saved.insert(
            (outcome.pair, 1),
            Saved {
                points: outcome.points[1],
                load: outcome.load,
            },
        );
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
    for pair in 0..pairs {
        let valid = records
            .get(&(pair, 0))
            .zip(records.get(&(pair, 1)))
            .and_then(|((_, left), (_, right))| {
                (!void(*left, ctrl, max_load) && !void(*right, ctrl, max_load))
                    .then_some((left.points, right.points))
            });
        valid.map_or_else(|| missing.push(pair), |points| game_points.push(points));
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
    println!("{header}");
    Ok(())
}
