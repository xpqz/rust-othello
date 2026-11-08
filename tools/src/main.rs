use std::process::exit;

use othello::agents::{Agent, greedy_agent, random_agent, search_agent};
use othello::eval::evaluate;
use othello::rng::Rng;
use othello::search::{SearchConfig, SearchLimits};
use othello_tools::baselines::{PHASE2_DEPTH, phase2_config};
use othello_tools::harness::{GameLog, play_game, points};
use othello_tools::positions;
use othello_tools::stats::{Interval, RESAMPLES, report_with};

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((command, rest)) if command == "match" => match_command(rest),
        _ => Err("usage: othello-tools match --a NAME --b NAME [--pairs N] [--seed S]".into()),
    }
}

fn make_agent(name: &str, seed: u64) -> Result<Box<dyn Agent>, String> {
    match name {
        "random" => Ok(Box::new(random_agent(seed))),
        "greedy" => Ok(Box::new(greedy_agent())),
        "phase2" => Ok(Box::new(search_agent(
            phase2_config(),
            SearchLimits {
                max_depth: PHASE2_DEPTH,
            },
        ))),
        _ => {
            let depth = name
                .strip_prefix("ab:")
                .ok_or_else(|| format!("unknown agent: {name}"))?
                .parse::<u32>()
                .map_err(|_| format!("unknown agent: {name}"))?;
            if depth == 0 || depth > 10 {
                return Err(format!("unknown agent: {name}"));
            }
            let config = SearchConfig {
                eval: evaluate,
                endgame_threshold: 12,
            };
            Ok(Box::new(search_agent(
                config,
                SearchLimits { max_depth: depth },
            )))
        }
    }
}

fn describe(name: &str) -> String {
    match name {
        "random" => "random".into(),
        "greedy" => "greedy (disc-greedy)".into(),
        "phase2" => format!("phase2 (eval_phase2, depth {PHASE2_DEPTH}, threshold 12)"),
        other => format!("{other} (evaluate, depth {}, threshold 12)", &other[3..]),
    }
}

fn stream(seed: u64, pair: usize, game: u64, side: u64) -> u64 {
    let id = ((pair as u64) << 2) | (game << 1) | side;
    Rng::new(Rng::new(seed).next_u64() ^ id).next_u64()
}

fn match_command(args: &[String]) -> Result<(), String> {
    let mut openings_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bench/openings_validation.txt"
    )
    .to_string();
    let (mut a, mut b, mut pairs, mut seed) = (None, None, 100usize, 1u64);
    let mut index = 0;
    while index < args.len() {
        let value = args.get(index + 1).ok_or("missing option value")?;
        match args[index].as_str() {
            "--a" => a = Some(value.clone()),
            "--b" => b = Some(value.clone()),
            "--pairs" => pairs = value.parse().map_err(|_| "invalid pairs")?,
            "--seed" => seed = value.parse().map_err(|_| "invalid seed")?,
            "--openings" => openings_path = value.clone(),
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
            let mut black_agent = make_agent(black, black_seed)?;
            let mut white_agent = make_agent(white, white_seed)?;
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
    println!("a {}", describe(&a));
    println!("b {}", describe(&b));
    println!("seed {seed} resamples {RESAMPLES} evaluator-only");
    for line in logs {
        println!("{line}");
    }
    Ok(())
}
