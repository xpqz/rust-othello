use std::process::exit;

use othello::agents::{Agent, greedy_agent, random_agent, search_agent};
use othello::rng::Rng;
use othello::search::{NO_DEADLINE, SearchConfig, SearchLimits};
use othello_tools::baselines::{PHASE2_DEPTH, phase2_config};
use othello_tools::harness::{GameLog, play_game, points};
use othello_tools::positions;
use othello_tools::stats::{Interval, RESAMPLES, report_with};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((command, rest)) if command == "match" => match_command(rest),
        _ => Err("usage: othello-tools match --a NAME --b NAME [--pairs N] [--seed S]".into()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        exit(2);
    }
}

fn unknown(name: &str) -> String {
    format!("unknown agent: {name}")
}

fn parse_name(name: &str) -> Result<(&str, u32, Option<u64>), String> {
    let mut parts = name.split(':');
    let kind = parts.next().unwrap();
    let time = |parts: &mut std::str::Split<char>| match parts.next() {
        None => Ok(None),
        Some(text) => text.parse().map(Some).map_err(|_| unknown(name)),
    };
    let (depth, time) = match kind {
        "random" | "greedy" => (0, None),
        "phase2" => (PHASE2_DEPTH, time(&mut parts)?),
        "ab" => {
            let depth = parts
                .next()
                .ok_or_else(|| unknown(name))?
                .parse()
                .map_err(|_| unknown(name))?;
            if depth == 0 || depth > 10 {
                return Err(unknown(name));
            }
            (depth, time(&mut parts)?)
        }
        _ => return Err(unknown(name)),
    };
    if parts.next().is_some() {
        return Err(unknown(name));
    }
    Ok((kind, depth, time))
}

fn effective(depth: u32, time: Option<u64>, nodes: u64) -> Result<(u32, u64), String> {
    if nodes == 0 {
        return Ok((depth, time.unwrap_or(NO_DEADLINE)));
    }
    if time.is_some() {
        return Err("time control conflicts with --nodes".into());
    }
    Ok((64, NO_DEADLINE))
}

fn make_agent(name: &str, seed: u64, nodes: u64, ordering: bool) -> Result<Box<dyn Agent>, String> {
    let (kind, depth, time) = parse_name(name)?;
    let (depth, time) = effective(depth, time, nodes)?;
    let limits = SearchLimits::new(depth, time, nodes);
    match kind {
        "random" => Ok(Box::new(random_agent(seed))),
        "greedy" => Ok(Box::new(greedy_agent())),
        "phase2" => Ok(Box::new(search_agent(
            SearchConfig {
                ordering,
                ..phase2_config()
            },
            limits,
        ))),
        _ => Ok(Box::new(search_agent(
            SearchConfig {
                ordering,
                ..SearchConfig::standard()
            },
            limits,
        ))),
    }
}

fn describe(name: &str, nodes: u64) -> String {
    let (kind, depth, time) = parse_name(name).unwrap();
    match kind {
        "random" => "random".into(),
        "greedy" => "greedy (disc-greedy)".into(),
        _ => {
            let (depth, time) = effective(depth, time, nodes).unwrap();
            let eval = if kind == "phase2" {
                "eval_phase2"
            } else {
                "evaluate"
            };
            let time = if time == NO_DEADLINE {
                "none".to_string()
            } else {
                time.to_string()
            };
            format!("{name} ({eval}, depth {depth}, time {time}, nodes {nodes}, threshold 12)")
        }
    }
}

fn stream(seed: u64, pair: usize, game: u64, side: u64) -> u64 {
    Rng::new(Rng::new(seed).next_u64() ^ ((pair as u64) << 2 | (game << 1) | side)).next_u64()
}

fn match_command(args: &[String]) -> Result<(), String> {
    let mut openings_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bench/openings_validation.txt"
    )
    .to_string();
    let (mut a, mut b, mut pairs, mut seed, mut nodes) = (None, None, 100usize, 1u64, 0u64);
    let mut ordering = true;
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
            "--openings" => openings_path = value.clone(),
            "--nodes" => nodes = value.parse().map_err(|_| "invalid nodes")?,
            flag => return Err(format!("unknown option: {flag}")),
        }
        index += 2;
    }
    let a = a.ok_or("missing --a")?;
    let b = b.ok_or("missing --b")?;
    let openings = positions::load(&openings_path)?;
    if openings.is_empty() {
        return Err("no openings".into());
    }
    if pairs > openings.len() {
        return Err(format!("pairs {pairs} exceeds {} openings", openings.len()));
    }
    let mut game_points = Vec::with_capacity(pairs);
    let mut logs = Vec::with_capacity(pairs * 2);
    for (pair, opening) in openings.iter().enumerate().take(pairs) {
        let mut pair_points = [0.0, 0.0];
        for (game, (black, white)) in [(a.as_str(), b.as_str()), (b.as_str(), a.as_str())]
            .into_iter()
            .enumerate()
        {
            let black_seed = stream(seed, pair, game as u64, 0);
            let white_seed = stream(seed, pair, game as u64, 1);
            let mut black_agent = make_agent(black, black_seed, nodes, ordering)?;
            let mut white_agent = make_agent(white, white_seed, nodes, ordering)?;
            let result = play_game(&opening.squares, &mut *black_agent, &mut *white_agent)?;
            pair_points[game] = if game == 0 {
                points(result.score)
            } else {
                1.0 - points(result.score)
            };
            logs.push(
                GameLog {
                    run: seed,
                    pair,
                    game: game as u64,
                    agent_seeds: (black_seed, white_seed),
                    key: opening.key,
                    opening: opening.line(),
                    black: black.to_string(),
                    white: white.to_string(),
                    score: result.score,
                }
                .line(),
            );
        }
        game_points.push((pair_points[0], pair_points[1]));
    }
    let summary = report_with(&game_points, RESAMPLES, seed);
    let interval = match summary.interval {
        Interval::Insufficient => "insufficient".to_string(),
        Interval::Uninformative { low, high, .. } => {
            format!("[{low:.4}, {high:.4}] uninformative")
        }
        Interval::Bootstrap { low, high, .. } => format!("[{low:.4}, {high:.4}] bootstrap"),
    };
    let elo = if summary.interval.informative() {
        summary.elo.map_or("none".to_string(), |(low, high)| {
            format!("[{low:.2}, {high:.2}]")
        })
    } else {
        "none".to_string()
    };
    println!(
        "pairs {} games {} W {} D {} L {} mean {:.4} interval {interval} elo {elo}",
        summary.pairs, summary.games, summary.wins, summary.draws, summary.losses, summary.mean
    );
    println!("a {}", describe(&a, nodes));
    println!("b {}", describe(&b, nodes));
    println!("seed {seed} resamples {RESAMPLES} evaluator-only nodes {nodes} ordering {ordering}");
    for line in logs {
        println!("{line}");
    }
    Ok(())
}
