//! Exact values at 24, 20 and 14 empties for every game of a ladder journal,
//! and the paired statistics on them. Resumable: one journal record per game.

#[path = "support/edax.rs"]
mod edax;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use othello::board::{empty_count, legal_moves};
use othello::search::{SearchConfig, SearchLimits, search_tt};
use othello::tt::Table;
use othello_tools::journal::Journal;
use othello_tools::provenance::{file_hash, git_commit};
use othello_tools::stats::{RESAMPLES, report_with};

const CHECKPOINTS: [u32; 3] = [24, 20, 14];
const SOLVE_LEVEL: u32 = 60;

/// One game from the engine's side: result, disc margin, exact values in
/// discs at each checkpoint.
#[derive(Clone, Debug, PartialEq)]
struct Game {
    points: f64,
    discs: i32,
    exact: [i32; 3],
}

fn field<'a>(line: &'a str, key: &str) -> Result<&'a str, String> {
    line.split_whitespace()
        .skip_while(|part| *part != key)
        .nth(1)
        .ok_or_else(|| format!("missing {key}"))
}

fn number<T: std::str::FromStr>(line: &str, key: &str) -> Result<T, String> {
    field(line, key)?
        .parse()
        .map_err(|_| format!("invalid {key}"))
}

fn game_line(game: &Game) -> String {
    format!(
        "points {:.4} discs {:+} v24 {:+} v20 {:+} v14 {:+}",
        game.points, game.discs, game.exact[0], game.exact[1], game.exact[2]
    )
}

fn parse_game(line: &str) -> Result<Game, String> {
    Ok(Game {
        points: number(line, "points")?,
        discs: number(line, "discs")?,
        exact: [
            number(line, "v24")?,
            number(line, "v20")?,
            number(line, "v14")?,
        ],
    })
}

/// Solves the first position at or below each checkpoint where the mover has
/// a move; a game that ends earlier keeps its final margin. The value at 14
/// empties is also solved by our own search and must agree.
fn measure(edax_bin: &Path, id: &str, payload: &str) -> Result<Game, String> {
    if field(payload, "void")? != "0" {
        return Err(format!("{id}: void game in source journal"));
    }
    let engine_black = field(payload, "engine_black")? == "1";
    let discs: i32 = number(payload, "discs")?;
    let moves = payload.split_once(" moves ").ok_or("missing moves")?.1;
    let positions = edax::replay(moves)?;
    let mut exact = [discs; 3];
    for (slot, &limit) in CHECKPOINTS.iter().enumerate() {
        let Some(&(board, black)) = positions.iter().find(|(board, _)| {
            empty_count(*board) <= limit && legal_moves(board.me, board.opp) != 0
        }) else {
            continue;
        };
        let tag = format!("{}-{limit}", id.replace(':', "_"));
        let value = edax::solve(edax_bin, board, black, SOLVE_LEVEL, &tag)?.score;
        if limit == 14 {
            let mut table = Table::new(1 << 20);
            let ours = search_tt(
                board,
                SearchLimits::depth(64),
                &SearchConfig::standard(),
                &mut table,
            );
            if ours.score != value * 100 {
                return Err(format!(
                    "{id}: edax {value:+} and our solver {:+} disagree at 14 empties",
                    ours.score
                ));
            }
        }
        exact[slot] = if engine_black == black { value } else { -value };
    }
    Ok(Game {
        points: number(payload, "points")?,
        discs,
        exact,
    })
}

fn outcome(value: i32) -> f64 {
    match value.signum() {
        1 => 1.0,
        0 => 0.5,
        _ => 0.0,
    }
}

fn share(label: &str, games: &[(f64, f64)], seed: u64) -> String {
    let report = report_with(games, RESAMPLES, seed);
    let interval = match report.interval.bounds() {
        Some((low, high)) if report.interval.informative() => format!("[{low:.4}, {high:.4}]"),
        _ => "uninformative".into(),
    };
    format!(
        "{label} W {} D {} L {} mean {:.4} {interval}",
        report.wins, report.draws, report.losses, report.mean
    )
}

/// Paired statistics. Requires both games of every pair in `0..pairs`.
fn summarise(
    games: &BTreeMap<(usize, usize), Game>,
    pairs: usize,
    seed: u64,
) -> Result<Vec<String>, String> {
    let mut paired = Vec::new();
    for pair in 0..pairs {
        match (games.get(&(pair, 0)), games.get(&(pair, 1))) {
            (Some(first), Some(second)) => paired.push((first, second)),
            _ => return Err(format!("pair {pair} is incomplete")),
        }
    }
    if games.len() != 2 * pairs {
        return Err(format!("{} games for {pairs} requested pairs", games.len()));
    }
    let by = |value: &dyn Fn(&Game) -> f64| -> Vec<(f64, f64)> {
        paired.iter().map(|(a, b)| (value(a), value(b))).collect()
    };
    let mut lines = vec![share("final", &by(&|game| game.points), seed)];
    let margins: Vec<f64> = by(&|game| f64::from(game.discs))
        .iter()
        .map(|(a, b)| (a + b) / 2.0)
        .collect();
    let n = margins.len() as f64;
    let mean = margins.iter().sum::<f64>() / n;
    let variance = margins.iter().map(|m| (m - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    lines.push(format!(
        "discs mean {mean:+.2} pair_se {:.2} pairs {pairs}",
        (variance / n).sqrt()
    ));
    for (slot, limit) in CHECKPOINTS.iter().enumerate() {
        let label = format!("exact{limit}");
        lines.push(share(&label, &by(&|game| outcome(game.exact[slot])), seed));
    }
    for (stage, label) in ["24to20", "20to14", "14toend"].iter().enumerate() {
        let (mut up, mut down) = (0, 0);
        for game in games.values() {
            let before = outcome(game.exact[stage]);
            let after = game
                .exact
                .get(stage + 1)
                .map_or(game.points, |&v| outcome(v));
            up += usize::from(after > before);
            down += usize::from(after < before);
        }
        lines.push(format!(
            "change {label} for_engine {up} against_engine {down}"
        ));
    }
    Ok(lines)
}

fn run(args: &[String]) -> Result<(), String> {
    let (mut edax_bin, mut source, mut out) = (PathBuf::new(), String::new(), String::new());
    let (mut pairs, mut seed) = (100usize, 1u64);
    for pair in args.chunks(2) {
        let [flag, value] = pair else {
            return Err("missing option value".into());
        };
        match flag.as_str() {
            "--edax" => edax_bin = value.into(),
            "--journal" => source = value.clone(),
            "--out" => out = value.clone(),
            "--pairs" => pairs = value.parse().map_err(|_| "invalid pairs")?,
            "--seed" => seed = value.parse().map_err(|_| "invalid seed")?,
            _ => return Err(format!("unknown option: {flag}")),
        }
    }
    if edax_bin.as_os_str().is_empty() || source.is_empty() || out.is_empty() {
        return Err(
            "usage: edax_phase --edax BIN --journal IN --out OUT [--pairs N] [--seed S]".into(),
        );
    }
    let eval = edax_bin
        .parent()
        .ok_or("edax path has no parent")?
        .join("data/eval.dat");
    let header = format!(
        "header v1 phase source {source} hash {:016x} edax_sha256 {} eval_sha256 {} commit {} checkpoints 24,20,14 solve_level {SOLVE_LEVEL}",
        file_hash(&source)?,
        edax::sha256(&edax_bin)?,
        edax::sha256(&eval)?,
        git_commit()
    );
    let (mut journal, done) = Journal::open(Path::new(&out), &header)?;
    let text = fs::read_to_string(&source).map_err(|error| format!("{source}: {error}"))?;
    for line in text.lines().skip(1) {
        let (id, payload) = line.split_once('\t').ok_or("malformed journal line")?;
        if !done.contains(id) {
            let game = measure(&edax_bin, id, payload)?;
            journal.append(id, &game_line(&game))?;
        }
    }
    let mut games = BTreeMap::new();
    for record in journal.records() {
        let (pair, game) = record.id.split_once(':').ok_or("invalid journal id")?;
        let key = (
            pair.parse().map_err(|_| "invalid pair")?,
            game.parse().map_err(|_| "invalid game")?,
        );
        games.insert(key, parse_game(&record.line)?);
    }
    println!("phase seed {seed} resamples {RESAMPLES} {header}");
    for line in summarise(&games, pairs, seed)? {
        println!("{line}");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn game(points: f64, discs: i32, exact: [i32; 3]) -> Game {
        Game {
            points,
            discs,
            exact,
        }
    }

    fn fixture() -> BTreeMap<(usize, usize), Game> {
        // Margins differ game to game, every pair mean is +2; exact values
        // put pair 0 at one win and one loss, pair 1 at two draws.
        BTreeMap::from([
            ((0, 0), game(1.0, 20, [6, 6, 20])),
            ((0, 1), game(0.0, -16, [-4, -16, -16])),
            ((1, 0), game(1.0, 30, [0, 30, 30])),
            ((1, 1), game(0.0, -26, [0, -2, -26])),
        ])
    }

    #[test]
    fn pair_error() {
        let lines = summarise(&fixture(), 2, 1).unwrap();
        assert_eq!(lines[1], "discs mean +2.00 pair_se 0.00 pairs 2");
    }

    #[test]
    fn checkpoint_shares() {
        let lines = summarise(&fixture(), 2, 1).unwrap();
        assert!(lines[0].starts_with("final W 2 D 0 L 2 mean 0.5000"));
        assert!(lines[2].starts_with("exact24 W 1 D 2 L 1 mean 0.5000"));
        assert!(lines[3].starts_with("exact20 W 2 D 0 L 2 mean 0.5000"));
        assert_eq!(lines[5], "change 24to20 for_engine 1 against_engine 1");
        assert_eq!(lines[6], "change 20to14 for_engine 0 against_engine 0");
        assert_eq!(lines[7], "change 14toend for_engine 0 against_engine 0");
    }

    #[test]
    fn constant_pairs() {
        // Every pair mean equal: the interval has no spread to resample.
        assert!(summarise(&fixture(), 2, 1).unwrap()[0].ends_with("uninformative"));
    }

    #[test]
    fn incomplete_pair() {
        let mut games = fixture();
        games.remove(&(1, 1));
        assert!(summarise(&games, 2, 1).is_err());
        assert!(summarise(&fixture(), 1, 1).is_err());
    }

    #[test]
    fn record_roundtrip() {
        let original = game(0.5, 0, [-2, 0, 4]);
        assert_eq!(parse_game(&game_line(&original)).unwrap(), original);
    }
}
