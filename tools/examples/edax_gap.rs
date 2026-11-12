//! Nodes and speed at equal depth: our search against Edax on positions
//! taken from a ladder journal. Sequential and single-threaded.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use othello::board::{Board, empty_count, legal_moves, pass, play};
use othello::search::{SearchConfig, SearchLimits, search_tt};
use othello::tt::{MAX_ENTRIES, Table};

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

fn square(text: &str) -> Result<u8, String> {
    match text.as_bytes() {
        [file @ b'a'..=b'h', rank @ b'1'..=b'8'] => Ok((rank - b'1') * 8 + (file - b'a')),
        _ => Err(format!("invalid square {text}")),
    }
}

/// One position per game, at 30 + (k mod 11) empties for the k-th game in
/// id order, mover with a legal move; duplicates dropped.
fn positions(journal: &str, count: usize) -> Result<Vec<(Board, bool)>, String> {
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
        games.push((key, moves));
    }
    games.sort();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (k, (_, moves)) in games.iter().enumerate() {
        let target = 30 + (k % 11) as u32;
        let (mut board, mut black) = (Board::START, true);
        for mv in moves.split_whitespace() {
            board = if mv == "pass" {
                pass(board)
            } else {
                play(board, square(mv)?)
            };
            black = !black;
            if empty_count(board) == target && legal_moves(board.me, board.opp) != 0 {
                if seen.insert((board.me, board.opp)) {
                    out.push((board, black));
                }
                break;
            }
        }
        if out.len() == count {
            break;
        }
    }
    Ok(out)
}

struct EdaxLine {
    depth: String,
    score: i32,
    ms: u64,
    nodes: u64,
}

/// Edax prints `# | depth score time nodes N/s pv`; time is `[h:]m:ss.mmm`.
fn parse_edax(stdout: &str) -> Option<EdaxLine> {
    stdout.lines().find_map(|line| {
        let mut fields = line.split_once('|')?.1.split_whitespace();
        let depth = fields.next()?.to_string();
        let score = fields.next()?.parse().ok()?;
        let seconds = fields.next()?.split(':').try_fold(0.0, |total, part| {
            part.parse::<f64>().ok().map(|value| total * 60.0 + value)
        })?;
        let nodes = fields.next()?.parse().ok()?;
        Some(EdaxLine {
            depth,
            score,
            ms: (seconds * 1000.0).round() as u64,
            nodes,
        })
    })
}

fn edax(args: &Args, board: Board, black: bool, index: usize) -> Result<EdaxLine, String> {
    let (black_bits, white_bits) = if black {
        (board.me, board.opp)
    } else {
        (board.opp, board.me)
    };
    let mut text: String = (0..64)
        .map(|sq| match 1u64 << sq {
            bit if black_bits & bit != 0 => 'X',
            bit if white_bits & bit != 0 => 'O',
            _ => '-',
        })
        .collect();
    text.push_str(if black { " X;\n" } else { " O;\n" });
    let path = std::env::temp_dir().join(format!("othello-gap-{}-{index}.obf", std::process::id()));
    fs::write(&path, text).map_err(|error| error.to_string())?;
    let output = Command::new(&args.edax)
        .current_dir(args.edax.parent().ok_or("edax path has no parent")?)
        .args(["-solve", path.to_str().ok_or("non-utf8 temp path")?])
        .args([
            "-level",
            &args.level.to_string(),
            "-verbose",
            "1",
            "-n",
            "1",
        ])
        .args(["-book-usage", "off", "-eval-file", "data/eval.dat"])
        .output()
        .map_err(|error| format!("{}: {error}", args.edax.display()))?;
    let _ = fs::remove_file(&path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_edax(&stdout).ok_or_else(|| format!("no edax result for position {index}: {stdout}"))
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values.get(values.len() / 2).copied().unwrap_or(f64::NAN)
}

fn run(args: &Args) -> Result<(), String> {
    let config = SearchConfig::standard();
    let boards = positions(&args.journal, args.count)?;
    let (mut node_ratio, mut time_ratio, mut depths) = (Vec::new(), Vec::new(), Vec::new());
    let (mut our_nodes, mut our_us, mut edax_nodes, mut edax_ms) = (0u64, 0u128, 0u64, 0u64);
    let (mut timed_nodes, mut timed_us) = (0u64, 0u128);
    for (index, &(board, black)) in boards.iter().enumerate() {
        let mut table = Table::new(MAX_ENTRIES);
        let start = Instant::now();
        let fixed = search_tt(board, SearchLimits::depth(args.depth), &config, &mut table);
        let fixed_us = start.elapsed().as_micros();
        let mut table = Table::new(MAX_ENTRIES);
        let start = Instant::now();
        let timed = search_tt(
            board,
            SearchLimits::new(64, args.time, 0),
            &config,
            &mut table,
        );
        let elapsed_us = start.elapsed().as_micros();
        let theirs = edax(args, board, black, index)?;
        println!(
            "pos {index} empties {} ours nodes {} us {fixed_us} score {:+} timed depth {} nodes {} us {elapsed_us} edax depth {} nodes {} ms {} score {:+}",
            empty_count(board),
            fixed.nodes,
            fixed.score,
            timed.depth,
            timed.nodes,
            theirs.depth,
            theirs.nodes,
            theirs.ms,
            theirs.score
        );
        node_ratio.push(fixed.nodes as f64 / theirs.nodes as f64);
        time_ratio.push(fixed_us as f64 / 1000.0 / theirs.ms.max(1) as f64);
        depths.push(timed.depth as f64);
        our_nodes += fixed.nodes;
        our_us += fixed_us;
        edax_nodes += theirs.nodes;
        edax_ms += theirs.ms;
        timed_nodes += timed.nodes;
        timed_us += elapsed_us;
    }
    let ours_nps = our_nodes as f64 / (our_us as f64 / 1e6);
    let edax_nps = edax_nodes as f64 / (edax_ms as f64 / 1e3);
    println!(
        "summary positions {} depth {} level {} node_ratio median {:.2} total {:.2} time_ratio median {:.2} total {:.2} ours_nps {:.0} edax_nps {:.0} speed_ratio {:.2}",
        boards.len(),
        args.depth,
        args.level,
        median(&mut node_ratio),
        our_nodes as f64 / edax_nodes as f64,
        median(&mut time_ratio),
        our_us as f64 / 1000.0 / edax_ms as f64,
        ours_nps,
        edax_nps,
        edax_nps / ours_nps
    );
    println!(
        "timed ms {} median_depth {:.0} mean_depth {:.2} nps {:.0} fixed_ms_mean {:.1} extra_time_ratio {:.2}",
        args.time,
        median(&mut depths.clone()),
        depths.iter().sum::<f64>() / depths.len() as f64,
        timed_nodes as f64 / (timed_us as f64 / 1e6),
        our_us as f64 / 1000.0 / boards.len() as f64,
        timed_us as f64 / our_us as f64
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

    #[test]
    fn edax_line() {
        let text = " # | depth|score|  time  | nodes | N/s | pv\n---+---\n  1|   10   -04        0:01.250       1234567    987654 g8 H7\n";
        let line = parse_edax(text).unwrap();
        assert_eq!(
            (line.depth.as_str(), line.score, line.ms, line.nodes),
            ("10", -4, 1250, 1_234_567)
        );
        assert!(parse_edax("no result").is_none());
    }

    #[test]
    fn positions_spread() {
        let journal = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/tuning/edax_level10.journal"
        );
        let boards = positions(journal, 100).unwrap();
        assert_eq!(boards.len(), 100);
        for (k, &(board, _)) in boards.iter().enumerate() {
            assert!((30..=40).contains(&empty_count(board)), "position {k}");
            assert_ne!(legal_moves(board.me, board.opp), 0);
        }
        let again = positions(journal, 100).unwrap();
        assert!(
            boards
                .iter()
                .zip(&again)
                .all(|(a, b)| a.0 == b.0 && a.1 == b.1)
        );
    }
}
