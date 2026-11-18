//! Time analysis of a clocked or timed match journal for the clock gate
//! (`docs/design/time.md` § Gate): realized means, the pre-registered
//! equal-time condition, and the paired time difference with its
//! pair-bootstrap interval. For a pilot (one agent against itself) it also
//! prints C, the mean per side per game rounded down to a whole second.
//!
//! Every statistic comes from the same pairs the score report uses: the
//! journal must hold games 0 and 1 of every pair in its declared range
//! exactly once, none above its load limit. Anything else is refused.
//!
//! Usage: `clock_time <journal> --a NAME --b NAME --pairs N [--seed S]`;
//! the expected agents and pair count must match the journal header.

use std::collections::BTreeMap;
use std::path::Path;

use othello_tools::journal::{Record, read};
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

fn parsed<T: std::str::FromStr>(line: &str, key: &str, what: &str) -> Result<T, String> {
    field(line, key)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("{what}: no valid {key}"))
}

#[derive(Debug, PartialEq)]
struct Summary {
    pairs: usize,
    /// Charged ms per agent (a, b) for each pair, over both games.
    per_pair: Vec<[u128; 2]>,
    /// Longest charged ms in one game, per agent.
    longest: [u64; 2],
    /// `flags[agent][colour]`.
    flags: [[usize; 2]; 2],
}

impl Summary {
    fn total(&self, agent: usize) -> u128 {
        self.per_pair.iter().map(|p| p[agent]).sum()
    }

    /// a's total at most 1.05 × b's, in integers: 20·a ≤ 21·b.
    fn equal_time(&self) -> bool {
        20 * self.total(0) <= 21 * self.total(1)
    }

    /// Mean per side per game, rounded down to whole seconds, in ms.
    fn pilot_c(&self) -> u128 {
        let side_games = 4 * self.pairs as u128;
        (self.total(0) + self.total(1)) / side_games / 1000 * 1000
    }
}

/// Validates the journal against the expected agents and pair count, and
/// summarizes its times.
fn analyse(
    header: &str,
    records: &[Record],
    a: &str,
    b: &str,
    pairs: usize,
) -> Result<Summary, String> {
    for (key, want) in [("a", a), ("b", b), ("pairs", &pairs.to_string())] {
        let found = field(header, key).unwrap_or("");
        if found != want {
            return Err(format!("header {key} is {found}, expected {want}"));
        }
    }
    let control = field(header, "control").unwrap_or("");
    if !matches!(control, "time" | "clock") {
        return Err(format!("control {control} records no times"));
    }
    let max_load: f64 = parsed(header, "max_load", "header")?;
    let mut games: BTreeMap<(usize, usize), &str> = BTreeMap::new();
    for record in records {
        let id = &record.id;
        let (pair, game) = id
            .split_once(':')
            .ok_or(format!("record {id}: invalid id"))?;
        let key: (usize, usize) = (
            pair.parse()
                .map_err(|_| format!("record {id}: invalid pair"))?,
            game.parse()
                .map_err(|_| format!("record {id}: invalid game"))?,
        );
        if key.0 >= pairs || key.1 > 1 {
            return Err(format!("record {id}: outside the declared range"));
        }
        if games.insert(key, &record.line).is_some() {
            return Err(format!("record {id}: duplicate"));
        }
    }
    if games.len() != 2 * pairs {
        return Err(format!("{} of {} games present", games.len(), 2 * pairs));
    }
    let mut summary = Summary {
        pairs,
        per_pair: vec![[0; 2]; pairs],
        longest: [0; 2],
        flags: [[0; 2]; 2],
    };
    for (&(pair, game), line) in &games {
        let id = format!("record {pair}:{game}");
        let load: f64 = parsed(line, "load", &id)?;
        if load > max_load {
            return Err(format!("{id}: load {load:.3} above {max_load:.3}, void"));
        }
        // The flagging or charged side is `colour`; it is agent `colour ^ game`.
        for (colour, key) in ["black_ms", "white_ms"].into_iter().enumerate() {
            let charged: u64 = parsed(line, key, &id)?;
            summary.per_pair[pair][colour ^ game] += u128::from(charged);
            let longest = &mut summary.longest[colour ^ game];
            *longest = (*longest).max(charged);
        }
        let colour = match field(line, "flag") {
            Some("none") => None,
            Some("black") => Some(0),
            Some("white") => Some(1),
            _ => return Err(format!("{id}: invalid flag")),
        };
        if let Some(colour) = colour {
            summary.flags[colour ^ game][colour] += 1;
        }
    }
    Ok(summary)
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: clock_time <journal> --a NAME --b NAME --pairs N [--seed S]";
    let path = args.first().ok_or(usage)?;
    let option = |flag: &str| {
        let index = args.iter().position(|arg| arg == flag)?;
        args.get(index + 1).cloned()
    };
    let (a, b) = (option("--a").ok_or(usage)?, option("--b").ok_or(usage)?);
    let pairs: usize = option("--pairs")
        .and_then(|p| p.parse().ok())
        .ok_or(usage)?;
    let seed: u64 = option("--seed").map_or(Ok(1), |s| s.parse().map_err(|_| usage))?;
    let (header, records) = read(Path::new(path))?;
    let summary = analyse(&header, &records, &a, &b, pairs)?;
    let games = 2 * summary.pairs as u128;
    let mean = |agent: usize| summary.total(agent) as f64 / games as f64;
    println!("a {a}");
    println!("b {b}");
    println!("pairs {} games {games}", summary.pairs);
    println!("total_ms a {} b {}", summary.total(0), summary.total(1));
    println!("mean_ms_per_game a {:.1} b {:.1}", mean(0), mean(1));
    println!(
        "max_ms_per_game a {} b {}",
        summary.longest[0], summary.longest[1]
    );
    println!(
        "equal_time 20a <= 21b: ratio {:.4} {}",
        mean(0) / mean(1),
        if summary.equal_time() {
            "holds"
        } else {
            "fails"
        }
    );
    let diffs: Vec<f64> = summary
        .per_pair
        .iter()
        .map(|p| p[0] as f64 - p[1] as f64)
        .collect();
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
    let f = summary.flags;
    println!(
        "flags a black {} white {} b black {} white {}",
        f[0][0], f[0][1], f[1][0], f[1][1]
    );
    println!("pilot C_ms {}", summary.pilot_c());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str =
        "header v1 a A b B pairs 2 seed 1 nodes 0 ordering true control clock max_load 6.000";

    /// A record for `id` with black and white charged ms, a flag and a load.
    fn record(id: &str, ms: [u64; 2], flag: &str, load: &str) -> Record {
        Record {
            id: id.into(),
            line: format!(
                "score 0 points 0.5000 load {load} jobs 1 black_ms {} white_ms {} flag {flag}",
                ms[0], ms[1]
            ),
        }
    }

    fn complete() -> Vec<Record> {
        vec![
            record("0:0", [10, 20], "none", "6.000"),
            record("0:1", [30, 40], "white", "1.000"),
            record("1:0", [50, 60], "black", "2.000"),
            record("1:1", [70, 80], "none", "3.000"),
        ]
    }

    #[test]
    fn sums_by_agent() {
        let summary = analyse(HEADER, &complete(), "A", "B", 2).unwrap();
        // a is black in game 0, white in game 1.
        assert_eq!(
            summary.per_pair,
            vec![[10 + 40, 20 + 30], [50 + 80, 60 + 70]]
        );
        assert_eq!(summary.longest, [80, 70]);
        // 0:1 white flagged: agent a (white in game 1); 1:0 black: agent a.
        assert_eq!(summary.flags, [[1, 1], [0, 0]]);
        assert_eq!(summary.pilot_c(), 0);
    }

    #[test]
    fn refuses_bad_sets() {
        let mut duplicate = complete();
        duplicate[1] = record("0:0", [1, 1], "none", "1.000");
        let mut overloaded = complete();
        overloaded[2] = record("1:0", [50, 60], "none", "6.001");
        let extra = [complete(), vec![record("2:0", [1, 1], "none", "1.000")]].concat();
        let cases = [
            (duplicate, "duplicate"),
            (complete()[..3].to_vec(), "3 of 4 games"),
            (overloaded, "void"),
            (extra, "outside"),
        ];
        for (records, word) in cases {
            let error = analyse(HEADER, &records, "A", "B", 2).unwrap_err();
            assert!(error.contains(word), "{word}: {error}");
        }
        let error = analyse(HEADER, &complete(), "A", "C", 2).unwrap_err();
        assert!(error.contains("header b"), "{error}");
        assert!(analyse(HEADER, &complete(), "A", "B", 3).is_err());
        let depth = HEADER.replace("control clock", "control depth");
        assert!(analyse(&depth, &complete(), "A", "B", 2).is_err());
    }

    /// Exactly 5% more holds; one millisecond more fails.
    #[test]
    fn equal_time_inclusive() {
        let header = HEADER.replace("pairs 2", "pairs 1");
        let check = |a: u64, b: u64| {
            let records = [
                record("0:0", [a, b], "none", "1.000"),
                record("0:1", [0, 0], "none", "1.000"),
            ];
            analyse(&header, &records, "A", "B", 1).unwrap()
        };
        let exact = check(44_100_378, 42_000_360);
        assert_eq!((exact.total(0), exact.total(1)), (44_100_378, 42_000_360));
        assert!(exact.equal_time());
        assert!(!check(44_100_379, 42_000_360).equal_time());
    }

    #[test]
    fn pilot_rounds_down() {
        let header = HEADER.replace("pairs 2", "pairs 1");
        let records = [
            record("0:0", [52_999, 52_000], "none", "1.000"),
            record("0:1", [53_000, 51_000], "none", "1.000"),
        ];
        // Side mean (52,999 + 52,000 + 53,000 + 51,000) / 4 = 52,249.75.
        let summary = analyse(&header, &records, "A", "B", 1).unwrap();
        assert_eq!(summary.pilot_c(), 52_000);
    }
}
