//! Shared by the Edax measurement examples: journal replay and one-shot solves.

use std::fs;
use std::path::Path;
use std::process::Command;

use othello::board::{Board, pass, play};

pub fn square(text: &str) -> Result<u8, String> {
    match text.as_bytes() {
        [file @ b'a'..=b'h', rank @ b'1'..=b'8'] => Ok((rank - b'1') * 8 + (file - b'a')),
        _ => Err(format!("invalid square {text}")),
    }
}

/// Positions after each move of a journal `moves` field, with black-to-move.
pub fn replay(moves: &str) -> Result<Vec<(Board, bool)>, String> {
    let (mut board, mut black) = (Board::START, true);
    let mut positions = Vec::new();
    for mv in moves.split_whitespace() {
        board = if mv == "pass" {
            pass(board)
        } else {
            play(board, square(mv)?)
        };
        black = !black;
        positions.push((board, black));
    }
    Ok(positions)
}

pub fn sha256(path: &Path) -> Result<String, String> {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout);
    match text.split_whitespace().next() {
        Some(hash) if output.status.success() => Ok(hash.to_string()),
        _ => Err(format!("shasum failed for {}", path.display())),
    }
}

#[derive(Debug, PartialEq)]
pub struct Solve {
    /// Edax's depth column, e.g. `10`, `12@73%` or the empties for a solve.
    pub depth: String,
    /// Discs, from the mover's side.
    pub score: i32,
    pub ms: u64,
    pub nodes: u64,
}

/// Edax prints `# | depth score time nodes N/s pv`; time is `[h:]m:ss.mmm`.
pub fn parse_solve(stdout: &str) -> Option<Solve> {
    stdout.lines().find_map(|line| {
        let mut fields = line.split_once('|')?.1.split_whitespace();
        let depth = fields.next()?.to_string();
        let score = fields.next()?.parse().ok()?;
        let seconds = fields.next()?.split(':').try_fold(0.0, |total, part| {
            part.parse::<f64>().ok().map(|value| total * 60.0 + value)
        })?;
        let nodes = fields.next()?.parse().ok()?;
        Some(Solve {
            depth,
            score,
            ms: (seconds * 1000.0).round() as u64,
            nodes,
        })
    })
}

/// Runs `edax -solve` on one position, single-threaded, book off.
pub fn solve(
    edax: &Path,
    board: Board,
    black: bool,
    level: u32,
    tag: &str,
) -> Result<Solve, String> {
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
    let path = std::env::temp_dir().join(format!("othello-edax-{}-{tag}.obf", std::process::id()));
    fs::write(&path, text).map_err(|error| error.to_string())?;
    let output = Command::new(edax)
        .current_dir(edax.parent().ok_or("edax path has no parent")?)
        .args(["-solve", path.to_str().ok_or("non-utf8 temp path")?])
        .args(["-level", &level.to_string(), "-verbose", "1", "-n", "1"])
        .args(["-book-usage", "off", "-eval-file", "data/eval.dat"])
        .output()
        .map_err(|error| format!("{}: {error}", edax.display()))?;
    let _ = fs::remove_file(&path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!("edax failed on {tag}: {}", output.status));
    }
    parse_solve(&stdout).ok_or_else(|| format!("no edax result for {tag}: {stdout}"))
}
