//! Time analysis of a clocked or timed match journal for the clock gate
//! (`docs/design/time.md` § Gate): realized means, the pre-registered
//! equal-time condition, and the paired time difference with its
//! pair-bootstrap interval. For a pilot (one agent against itself) it also
//! prints C, the mean per side per game rounded down to a whole second.
//!
//! Usage: `clock_time <journal> [--seed S]`.

use std::collections::BTreeMap;
use std::path::Path;

use othello_tools::journal::read;
use othello_tools::stats::{Interval, RESAMPLES, bootstrap_ci};

/// The value after `key` in a whitespace-separated line.
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let mut parts = line.split_whitespace();
    while let Some(part) = parts.next() {
        if part == key {
            return parts.next();
        }
    }
    None
}

fn number(line: &str, key: &str, id: &str) -> Result<u64, String> {
    field(line, key)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("record {id}: no valid {key}"))
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .ok_or("usage: clock_time <journal> [--seed S]")?;
    let seed = match args.get(1).map(String::as_str) {
        Some("--seed") => args
            .get(2)
            .and_then(|s| s.parse().ok())
            .ok_or("invalid seed")?,
        _ => 1,
    };
    let (header, records) = read(Path::new(path))?;
    // Per pair: [a, b] charged ms over both games; a is black in game 0.
    let mut pairs: BTreeMap<usize, ([u64; 2], usize)> = BTreeMap::new();
    let (mut flags, mut longest) = ([[0usize; 2]; 2], [0u64; 2]);
    for record in &records {
        let (pair, game) = record.id.split_once(':').ok_or("invalid id")?;
        let (pair, game): (usize, usize) = (
            pair.parse().map_err(|_| "invalid pair")?,
            game.parse().map_err(|_| "invalid game")?,
        );
        let line = &record.line;
        let ms = [
            number(line, "black_ms", &record.id)?,
            number(line, "white_ms", &record.id)?,
        ];
        let entry = pairs.entry(pair).or_default();
        for (side, charged) in ms.into_iter().enumerate() {
            entry.0[side ^ game] += charged;
            longest[side ^ game] = longest[side ^ game].max(charged);
        }
        entry.1 += 1;
        // flags[agent][colour]: the flagging side is `colour`, agent `colour ^ game`.
        let colour = match field(line, "flag") {
            Some("none") => None,
            Some("black") => Some(0),
            Some("white") => Some(1),
            _ => return Err(format!("record {}: invalid flag", record.id)),
        };
        if let Some(colour) = colour {
            flags[colour ^ game][colour] += 1;
        }
    }
    let complete: Vec<[u64; 2]> = pairs.values().filter(|p| p.1 == 2).map(|p| p.0).collect();
    let games = 2 * complete.len() as u64;
    if games == 0 {
        return Err("no complete pairs".into());
    }
    let total = |side: usize| complete.iter().map(|p| u128::from(p[side])).sum::<u128>();
    let mean = |side: usize| total(side) as f64 / games as f64;
    println!("a {}", field(&header, "a").unwrap_or("?"));
    println!("b {}", field(&header, "b").unwrap_or("?"));
    println!(
        "pairs {} games {games} incomplete {}",
        complete.len(),
        pairs.len() - complete.len()
    );
    println!("mean_ms_per_game a {:.1} b {:.1}", mean(0), mean(1));
    println!("max_ms_per_game a {} b {}", longest[0], longest[1]);
    let ratio = mean(0) / mean(1);
    let holds = mean(0) <= 1.05 * mean(1);
    println!(
        "equal_time a <= 1.05 b: ratio {ratio:.4} {}",
        if holds { "holds" } else { "fails" }
    );
    let diffs: Vec<f64> = complete.iter().map(|p| p[0] as f64 - p[1] as f64).collect();
    // The score interval's degeneracy fallback is clamped to [0, 1], which
    // does not apply to milliseconds: only a bootstrap interval is shown.
    match bootstrap_ci(&diffs, RESAMPLES, seed) {
        Interval::Bootstrap { mean, low, high } => println!(
            "paired_diff_ms_per_pair a-b mean {mean:.1} interval [{low:.1}, {high:.1}] resamples {RESAMPLES} seed {seed}"
        ),
        _ => println!(
            "paired_diff_ms_per_pair a-b mean {:.1} interval unavailable (fewer than 10 pairs or no variance)",
            diffs.iter().sum::<f64>() / diffs.len() as f64
        ),
    }
    println!(
        "flags a black {} white {} b black {} white {}",
        flags[0][0], flags[0][1], flags[1][0], flags[1][1]
    );
    let side_mean = (total(0) + total(1)) / (2 * u128::from(games));
    println!(
        "pilot side_mean_ms {side_mean} C_ms {}",
        side_mean / 1000 * 1000
    );
    Ok(())
}
