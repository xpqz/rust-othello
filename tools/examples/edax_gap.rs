//! Nodes and speed at equal depth: our search against Edax on positions
//! taken from a ladder journal. Sequential and single-threaded.

#[path = "support/edax.rs"]
mod edax;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use othello::board::{Board, empty_count, legal_moves};
use othello::search::{SearchConfig, SearchLimits, search_tt};
use othello::tt::{MAX_ENTRIES, Table};
use othello_tools::provenance::{file_hash, git_commit};

struct Args {
    edax: PathBuf,
    journal: String,
    count: usize,
    depth: u32,
    level: u32,
    time: u64,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        edax: PathBuf::new(),
        journal: concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/tuning/edax_level10.journal"
        )
        .into(),
        count: 100,
        depth: 10,
        level: 10,
        time: 3000,
    };
    for pair in args.chunks(2) {
        let [flag, value] = pair else {
            return Err("missing option value".into());
        };
        let number = || value.parse::<u64>().map_err(|_| format!("invalid {flag}"));
        match flag.as_str() {
            "--edax" => a.edax = value.into(),
            "--journal" => a.journal = value.clone(),
            "--count" => a.count = number()? as usize,
            "--depth" => a.depth = number()? as u32,
            "--level" => a.level = number()? as u32,
            "--time" => a.time = number()?,
            _ => return Err(format!("unknown option: {flag}")),
        }
    }
    if a.edax.as_os_str().is_empty() {
        return Err("missing --edax".into());
    }
    Ok(a)
}

struct Position {
    id: String,
    board: Board,
    black: bool,
}

/// One position per game, at 30 + (k mod 11) empties for the k-th game in
/// id order, mover with a legal move; duplicates dropped. Exactly `count`.
fn positions(journal: &str, count: usize) -> Result<Vec<Position>, String> {
    let text = fs::read_to_string(journal).map_err(|error| format!("{journal}: {error}"))?;
    let mut games = Vec::new();
    for line in text.lines().skip(1) {
        let (id, payload) = line.split_once('\t').ok_or("malformed journal line")?;
        let (pair, game) = id.split_once(':').ok_or("invalid journal id")?;
        let key: (usize, usize) = (
            pair.parse().map_err(|_| "invalid pair")?,
            game.parse().map_err(|_| "invalid game")?,
        );
        let moves = payload.split_once(" moves ").ok_or("missing moves")?.1;
        games.push((key, id, moves));
    }
    games.sort();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (k, (_, id, moves)) in games.iter().enumerate() {
        let target = 30 + (k % 11) as u32;
        let found = edax::replay(moves)?.into_iter().find(|(board, _)| {
            empty_count(*board) == target && legal_moves(board.me, board.opp) != 0
        });
        if let Some((board, black)) = found
            && seen.insert((board.me, board.opp))
        {
            out.push(Position {
                id: id.to_string(),
                board,
                black,
            });
        }
        if out.len() == count {
            return Ok(out);
        }
    }
    Err(format!(
        "only {} of {count} positions in {journal}",
        out.len()
    ))
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values.get(values.len() / 2).copied().unwrap_or(f64::NAN)
}

fn run(args: &Args) -> Result<(), String> {
    let config = SearchConfig::standard();
    let boards = positions(&args.journal, args.count)?;
    let date = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .map_err(|error| error.to_string())?;
    println!(
        "gap v1 commit {} date {} edax {} edax_sha256 {} eval_sha256 {} journal {} hash {:016x} positions {} depth {} level {} time {}",
        git_commit(),
        String::from_utf8_lossy(&date.stdout).trim(),
        args.edax.display(),
        edax::sha256(&args.edax)?,
        edax::sha256(
            &args
                .edax
                .parent()
                .ok_or("edax path has no parent")?
                .join("data/eval.dat")
        )?,
        args.journal,
        file_hash(&args.journal)?,
        boards.len(),
        args.depth,
        args.level,
        args.time
    );
    let (mut node_ratio, mut time_ratio) = (Vec::new(), Vec::new());
    let (mut our_nodes, mut our_us, mut edax_nodes, mut edax_ms) = (0u64, 0u128, 0u64, 0u64);
    let (mut below, mut at, mut above) = (0, 0, 0);
    for (index, position) in boards.iter().enumerate() {
        let board = position.board;
        let mut table = Table::new(MAX_ENTRIES);
        let start = Instant::now();
        let fixed = search_tt(board, SearchLimits::depth(args.depth), &config, &mut table);
        let fixed_us = start.elapsed().as_micros();
        let mut table = Table::new(MAX_ENTRIES);
        let timed = search_tt(
            board,
            SearchLimits::new(64, args.time, 0),
            &config,
            &mut table,
        );
        let theirs = edax::solve(
            &args.edax,
            board,
            position.black,
            args.level,
            &index.to_string(),
        )?;
        if theirs.depth != args.depth.to_string() {
            return Err(format!(
                "position {index}: edax searched {}, expected non-selective depth {}",
                theirs.depth, args.depth
            ));
        }
        println!(
            "pos {index} game {} board {:016x} {:016x} empties {} ours nodes {} us {fixed_us} score {:+} timed_depth {} edax depth {} nodes {} ms {} score {:+}",
            position.id,
            board.me,
            board.opp,
            empty_count(board),
            fixed.nodes,
            fixed.score,
            timed.depth,
            theirs.depth,
            theirs.nodes,
            theirs.ms,
            theirs.score
        );
        node_ratio.push(fixed.nodes as f64 / theirs.nodes as f64);
        time_ratio.push(fixed_us as f64 / 1000.0 / theirs.ms.max(1) as f64);
        match timed.depth.cmp(&args.depth) {
            std::cmp::Ordering::Less => below += 1,
            std::cmp::Ordering::Equal => at += 1,
            std::cmp::Ordering::Greater => above += 1,
        }
        our_nodes += fixed.nodes;
        our_us += fixed_us;
        edax_nodes += theirs.nodes;
        edax_ms += theirs.ms;
    }
    let ours_nps = our_nodes as f64 / (our_us as f64 / 1e6);
    let edax_nps = edax_nodes as f64 / (edax_ms as f64 / 1e3);
    let fixed_ms = our_us as f64 / 1000.0 / boards.len() as f64;
    println!(
        "summary positions {} node_ratio median {:.2} total {:.2} time_ratio median {:.2} total {:.2} ours_nps {:.0} edax_nps {:.0} speed_ratio {:.2}",
        boards.len(),
        median(&mut node_ratio),
        our_nodes as f64 / edax_nodes as f64,
        median(&mut time_ratio),
        our_us as f64 / 1000.0 / edax_ms as f64,
        ours_nps,
        edax_nps,
        edax_nps / ours_nps
    );
    println!(
        "budget ms {} fixed_ms_mean {fixed_ms:.1} budget_ratio {:.2} timed_depth below {below} at {at} above {above}",
        args.time,
        args.time as f64 / fixed_ms
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(message) = parse_args(&args).and_then(|args| run(&args)) {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOURNAL: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/tuning/edax_level10.journal"
    );

    #[test]
    fn edax_line() {
        let text = " # | depth|score|  time  | nodes | N/s | pv\n---+---\n  1|   10   -04        0:01.250       1234567    987654 g8 H7\n";
        let line = edax::parse_solve(text).unwrap();
        assert_eq!(
            (line.depth.as_str(), line.score, line.ms, line.nodes),
            ("10", -4, 1250, 1_234_567)
        );
        assert!(edax::parse_solve("no result").is_none());
    }

    #[test]
    fn positions_spread() {
        let boards = positions(JOURNAL, 100).unwrap();
        assert_eq!(boards.len(), 100);
        for position in &boards {
            let board = position.board;
            assert!((30..=40).contains(&empty_count(board)), "{}", position.id);
            assert_ne!(legal_moves(board.me, board.opp), 0);
        }
        let again = positions(JOURNAL, 100).unwrap();
        assert!(boards.iter().zip(&again).all(|(a, b)| a.board == b.board));
    }

    #[test]
    fn positions_short() {
        assert!(positions(JOURNAL, 101).is_err());
    }
}
