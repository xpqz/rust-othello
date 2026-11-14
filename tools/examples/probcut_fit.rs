//! ProbCut v2 calibration (docs/design/probcut2.md § Calibration data).
//!
//! `collect`: unit `n` picks a WTHOR game and a root position in it, runs a
//! plain alpha-beta search of `SAMPLE_DEPTH` from the root with the engine's
//! move ordering, and keeps one node it visits with remaining depth >= 3,
//! chosen uniformly (reservoir). For that node it records the static value and
//! the full-width search value at every depth 1..=D, table cleared once per
//! node: D is `--depth`, or `--deep-depth` for every `--deep-every`th unit.
//! Units are deterministic in `n` and `--seed`; one journal line per unit, so
//! the job resumes by re-running the same command, and a larger `--nodes`
//! extends a finished run. Progress is reported every `--progress` seconds.
//!
//! `fit`: reads a `collect` journal; folds by game, canonical deduplication,
//! development units out of the audit fold; fits the design's σ forms on the
//! fit fold; selects a rule, form and rectangle on the selection fold with the
//! model quantized exactly as the engine evaluates it; and, with `--audit`
//! (once, after the choice is frozen), checks it on the audit fold and prints
//! the audited `ProbCut` fields.
//!
//! usage: probcut_fit collect --out FILE [--nodes 12000] [--depth 14]
//!        [--deep-depth 16] [--deep-every 8] [--jobs 6] [--seed 145]
//!        [--progress 30]
//!        probcut_fit fit --in FILE --dev FILE [--audit]

use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::path::Path;
use std::str::FromStr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use othello::board::{Board, empty_count, final_score, legal_moves, pass, play};
use othello::pattern::evaluate;
use othello::score::terminal_score;
use othello::search::{SearchConfig, SearchLimits, ordered_moves, search_tt};
use othello::tt::{MAX_ENTRIES, Table};
use othello_tools::journal::Journal;
use othello_tools::provenance::{file_hash, git_commit};

const WTHOR: &str = "data/wthor";
const SAMPLE_DEPTH: u32 = 8;
/// Root positions are drawn with this many empties, uniformly.
const ROOT_EMPTIES: std::ops::RangeInclusive<u32> = 26..=58;
/// Sampled nodes keep at least this many empties.
const MIN_EMPTIES: u32 = 16;
/// Games whose hashed index falls below this share (of 10) are the fit split.
const FIT_TENTHS: u64 = 7;

fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = mix(self.0);
        self.0 % n
    }
}

fn decode(byte: u8) -> Option<u8> {
    let (column, row) = (byte % 10, byte / 10);
    ((1..=8).contains(&column) && (1..=8).contains(&row)).then(|| (row - 1) * 8 + column - 1)
}

/// Every WTHOR game as its move list, files in name order. Returns the games
/// and a hash of the file list and sizes for the journal header.
fn wthor(root: &Path) -> Result<(Vec<Vec<u8>>, u64), String> {
    let dir = root.join(WTHOR);
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wtb"))
        })
        .collect();
    names.sort();
    let (mut games, mut hash) = (Vec::new(), 0u64);
    for name in names {
        let bytes = std::fs::read(&name).map_err(|error| error.to_string())?;
        hash = mix(hash ^ bytes.len() as u64 ^ file_hash(&name.to_string_lossy())?);
        for record in bytes.get(16..).unwrap_or_default().as_chunks::<68>().0 {
            let moves = record[8..].iter().take_while(|&&byte| byte != 0);
            games.push(moves.map(|&byte| decode(byte).unwrap_or(u8::MAX)).collect());
        }
    }
    Ok((games, hash))
}

/// The game's position with exactly `empties` empties and a legal move, if any.
fn position(moves: &[u8], empties: u32) -> Option<Board> {
    let mut board = Board::START;
    for &square in moves {
        if legal_moves(board.me, board.opp) == 0 {
            board = pass(board);
        }
        let movable = legal_moves(board.me, board.opp);
        if empty_count(board) == empties && movable != 0 {
            return Some(board);
        }
        if square >= 64 || movable & (1u64 << square) == 0 {
            return None;
        }
        board = play(board, square);
    }
    None
}

struct Sampler {
    rng: Rng,
    seen: u64,
    pick: Option<Board>,
}

fn order_eval(depth: u32) -> Option<fn(Board) -> i32> {
    (depth >= 4).then_some(evaluate as fn(Board) -> i32)
}

impl Sampler {
    fn search(&mut self, board: Board, depth: u32, mut alpha: i32, beta: i32) -> i32 {
        let moves = legal_moves(board.me, board.opp);
        if moves == 0 {
            if legal_moves(board.opp, board.me) == 0 {
                return terminal_score(final_score(board));
            }
            return -self.search(pass(board), depth, -beta, -alpha);
        }
        if depth == 0 {
            return evaluate(board);
        }
        if depth >= 3 && empty_count(board) >= MIN_EMPTIES {
            self.seen += 1;
            if self.rng.below(self.seen) == 0 {
                self.pick = Some(board);
            }
        }
        let (order, len) = ordered_moves(board, moves, 0, None, order_eval(depth));
        let mut best = i32::MIN;
        for &square in &order[..len] {
            let value = -self.search(play(board, square), depth - 1, -beta, -alpha);
            best = best.max(value);
            alpha = alpha.max(value);
            if alpha >= beta {
                break;
            }
        }
        best
    }
}

/// Unit `n`: (game index, sampled node). Redraws until a game has a root.
fn unit(games: &[Vec<u8>], seed: u64, n: usize) -> (usize, Board) {
    let mut rng = Rng(mix(seed ^ mix(n as u64)));
    loop {
        let game = rng.below(games.len() as u64) as usize;
        let span = u64::from(ROOT_EMPTIES.end() - ROOT_EMPTIES.start() + 1);
        let empties = ROOT_EMPTIES.start() + rng.below(span) as u32;
        let Some(root) = position(&games[game], empties) else {
            continue;
        };
        let mut sampler = Sampler {
            rng: Rng(rng.0),
            seen: 0,
            pick: None,
        };
        sampler.search(root, SAMPLE_DEPTH, -i32::MAX, i32::MAX);
        if let Some(node) = sampler.pick {
            return (game, node);
        }
        rng.0 = sampler.rng.0;
    }
}

fn measure(node: Board, depth: u32, table: &mut Table) -> (Vec<i32>, u64) {
    let config = SearchConfig {
        endgame_threshold: 0,
        ..SearchConfig::standard()
    };
    table.clear();
    let (mut values, mut nodes) = (vec![evaluate(node)], 0);
    for d in 1..=depth {
        let result = search_tt(node, SearchLimits::depth(d), &config, table);
        values.push(result.score);
        nodes += result.nodes;
    }
    (values, nodes)
}

/// Parses `--name value` and `--flag` options; an unknown, repeated or
/// value-missing option is an error.
fn options(
    args: &[String],
    valued: &[&str],
    flags: &[&str],
) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    let mut rest = args.iter();
    while let Some(name) = rest.next() {
        let value = if flags.contains(&name.as_str()) {
            String::new()
        } else if valued.contains(&name.as_str()) {
            match rest.next() {
                Some(value) if !value.starts_with("--") => value.clone(),
                _ => return Err(format!("{name} needs a value")),
            }
        } else {
            return Err(format!("unknown option {name}"));
        };
        if map.insert(name.clone(), value).is_some() {
            return Err(format!("{name} given more than once"));
        }
    }
    Ok(map)
}

/// The option's value, or `default`, checked to lie in `range`.
fn ranged<T: FromStr + PartialOrd + Display + Copy>(
    map: &HashMap<String, String>,
    name: &str,
    default: T,
    range: std::ops::RangeInclusive<T>,
) -> Result<T, String> {
    let value = match map.get(name) {
        Some(text) => text.parse().map_err(|_| format!("invalid {name} {text}"))?,
        None => default,
    };
    if range.contains(&value) {
        Ok(value)
    } else {
        Err(format!(
            "{name} {value} outside {}..={}",
            range.start(),
            range.end()
        ))
    }
}

fn collect(args: &[String]) -> Result<(), String> {
    let usage = "usage: probcut_fit collect --out FILE [--nodes N] [--depth D] [--deep-depth D] [--deep-every K] [--jobs J] [--seed S] [--progress SECONDS]";
    let map = options(
        args,
        &[
            "--out",
            "--nodes",
            "--depth",
            "--deep-depth",
            "--deep-every",
            "--jobs",
            "--seed",
            "--progress",
        ],
        &[],
    )
    .map_err(|error| format!("{error}\n{usage}"))?;
    let out = map.get("--out").ok_or(usage)?.clone();
    let nodes: usize = ranged(&map, "--nodes", 12_000, 0..=1_000_000)?;
    let depth: u32 = ranged(&map, "--depth", 14, 1..=30)?;
    let deep: u32 = ranged(&map, "--deep-depth", 16.max(depth), depth..=30)?;
    let every: usize = ranged(&map, "--deep-every", 8, 1..=1_000_000)?;
    let jobs: usize = ranged(&map, "--jobs", 6, 1..=64)?;
    let seed: u64 = ranged(&map, "--seed", 145, 0..=u64::MAX)?;
    let progress: u64 = ranged(&map, "--progress", 30, 1..=86_400)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let (games, data) = wthor(&root)?;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    // `--nodes`, `--jobs` and `--progress` are left out: they change no unit.
    let header = format!(
        "header v1 probcut_fit collect depth {depth} deep_depth {deep} deep_every {every} seed {seed} sample_depth {SAMPLE_DEPTH} root_empties {}..={} min_empties {MIN_EMPTIES} games {} data {data:016x} commit {} binary {:016x}",
        ROOT_EMPTIES.start(),
        ROOT_EMPTIES.end(),
        games.len(),
        git_commit(),
        file_hash(&exe.to_string_lossy())?
    );
    let (journal, done) = Journal::open(Path::new(&out), &header)?;
    let pending: Vec<usize> = (0..nodes)
        .filter(|n| !done.contains(&n.to_string()))
        .collect();
    let total = pending.len();
    let (journal, next, finished, stop) = (
        Mutex::new(journal),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicBool::new(false),
    );
    let start = Instant::now();
    eprintln!("probcut_fit collect: {total} of {nodes} units to do, {jobs} jobs");
    let report = |count: usize| {
        let elapsed = start.elapsed().as_secs_f64();
        let eta = if count > 0 {
            format!(
                " eta {:.0}s",
                elapsed / count as f64 * (total - count) as f64
            )
        } else {
            String::new()
        };
        eprintln!("progress {count}/{total} elapsed {elapsed:.0}s{eta}");
    };
    std::thread::scope(|scope| -> Result<(), String> {
        scope.spawn(|| {
            let mut last = Instant::now();
            while !stop.load(Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
                if last.elapsed() >= Duration::from_secs(progress) {
                    last = Instant::now();
                    report(finished.load(Relaxed));
                }
            }
        });
        let workers: Vec<_> = (0..jobs)
            .map(|_| {
                scope.spawn(|| -> Result<(), String> {
                    let mut table = Table::new(MAX_ENTRIES);
                    while let Some(&n) = pending.get(next.fetch_add(1, Relaxed)) {
                        let (game, node) = unit(&games, seed, n);
                        let max = if n % every == 0 { deep } else { depth };
                        let begin = Instant::now();
                        let (values, searched) = measure(node, max, &mut table);
                        let split = if mix(game as u64 ^ seed) % 10 < FIT_TENTHS { "fit" } else { "held" };
                        let list: Vec<String> = values.iter().map(i32::to_string).collect();
                        let line = format!(
                            "game {game} split {split} empties {} board {:016x}:{:016x} depth {max} nodes {searched} ms {:.0} values {}",
                            empty_count(node),
                            node.me,
                            node.opp,
                            begin.elapsed().as_secs_f64() * 1000.0,
                            list.join(",")
                        );
                        journal.lock().unwrap().append(&n.to_string(), &line)?;
                        finished.fetch_add(1, Relaxed);
                    }
                    Ok(())
                })
            })
            .collect();
        let result = workers
            .into_iter()
            .try_for_each(|worker| worker.join().map_err(|_| "worker panicked".to_string())?);
        stop.store(true, Relaxed);
        result
    })?;
    report(finished.load(Relaxed));
    let journal = journal.into_inner().unwrap();
    let records = journal.records();
    let ms: f64 = records
        .iter()
        .filter_map(|record| field(&record.line, "ms")?.parse::<f64>().ok())
        .sum();
    // `unit_wall_s` sums each unit's measurement wall time (sampling excluded).
    println!(
        "probcut_fit collect units {} unit_wall_s {:.0} wall_s {:.0}",
        records.len(),
        ms / 1000.0,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

struct Sample {
    id: usize,
    game: usize,
    board: Board,
    empties: u32,
    values: Vec<i32>,
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let mut words = line.split_whitespace();
    while let Some(word) = words.next() {
        if word == name {
            return words.next();
        }
    }
    None
}

/// Every record of a `collect` journal; a record whose value count is not
/// its depth plus one is an error (values are completed searches only).
fn samples(path: &Path) -> Result<Vec<Sample>, String> {
    let (_, records) = othello_tools::journal::read(path)?;
    records
        .iter()
        .map(|record| {
            let line = &record.line;
            let number = |name: &str| field(line, name).and_then(|v| v.parse::<u32>().ok());
            let values: Vec<i32> = field(line, "values")
                .ok_or("no values")?
                .split(',')
                .map(|value| value.parse().map_err(|_| "bad value".to_string()))
                .collect::<Result<_, _>>()?;
            let depth = number("depth").ok_or("bad depth")?;
            if values.len() != depth as usize + 1 {
                return Err(format!(
                    "unit {}: {} values for depth {depth}",
                    record.id,
                    values.len()
                ));
            }
            let (me, opp) = field(line, "board")
                .and_then(|b| b.split_once(':'))
                .and_then(|(me, opp)| {
                    Some((
                        u64::from_str_radix(me, 16).ok()?,
                        u64::from_str_radix(opp, 16).ok()?,
                    ))
                })
                .ok_or("bad board")?;
            Ok(Sample {
                id: record.id.parse().map_err(|_| "bad unit id")?,
                game: field(line, "game")
                    .and_then(|g| g.parse().ok())
                    .ok_or("bad game")?,
                board: Board { me, opp },
                empties: number("empties").ok_or("bad empties")?,
                values,
            })
        })
        .collect()
}

#[cfg(test)]
fn flip_vertical(x: u64) -> u64 {
    x.swap_bytes()
}

fn mirror_horizontal(x: u64) -> u64 {
    u64::from_le_bytes(x.to_le_bytes().map(u8::reverse_bits))
}

fn transpose(mut x: u64) -> u64 {
    let mut t = 0x0f0f_0f0f_0000_0000 & (x ^ (x << 28));
    x ^= t ^ (t >> 28);
    t = 0x3333_0000_3333_0000 & (x ^ (x << 14));
    x ^= t ^ (t >> 14);
    t = 0x5500_5500_5500_5500 & (x ^ (x << 7));
    x ^ t ^ (t >> 7)
}

/// The least of the eight symmetric images of the position (D4).
fn canonical(board: Board) -> (u64, u64) {
    let mut best = (u64::MAX, u64::MAX);
    let (mut me, mut opp) = (board.me, board.opp);
    for turn in 0..8 {
        best = best.min((me, opp));
        (me, opp) = if turn % 2 == 0 {
            (mirror_horizontal(me), mirror_horizontal(opp))
        } else {
            (transpose(me), transpose(opp))
        };
    }
    best
}

/// Fold by game: 0 fit (60%), 1 selection (20%), 2 audit (20%).
fn fold(game: usize) -> usize {
    match mix(game as u64 ^ 0x5eed_f01d) % 10 {
        0..=5 => 0,
        6 | 7 => 1,
        _ => 2,
    }
}

/// Samples in game folds without repeated canonical positions across folds
/// (the lowest unit id is kept); development units are never in the audit fold.
fn distinct(mut samples: Vec<Sample>, development: &HashSet<usize>) -> Vec<(usize, Sample)> {
    samples.sort_by_key(|sample| sample.id);
    let mut seen = HashSet::new();
    samples
        .into_iter()
        .filter(|sample| seen.insert(canonical(sample.board)))
        .map(|sample| (fold(sample.game), sample))
        .filter(|(fold, sample)| !(*fold == 2 && development.contains(&sample.id)))
        .collect()
}

/// Empties buckets (inclusive) for cells.
const BUCKETS: [(u32, u32); 5] = [(16, 23), (24, 31), (32, 39), (40, 47), (48, 60)];
const DEPTHS: std::ops::RangeInclusive<u32> = 3..=16;
/// The engine's solve threshold: ProbCut acts from two empties above it.
const THRESHOLD: u32 = 18;
const MAX_EMPTIES: u32 = 56;
/// Cell acceptance: positions, σ floor, proxy error bound.
const MIN_POSITIONS: usize = 60;
const FLOOR: f64 = 0.95;
const MAX_ERROR: f64 = 0.15;
/// The proxy check's `t`, in hundredths of σ.
const CHECK_T: i64 = 150;
/// Runtime representation limits (design § The check).
const SIGMA_MAX: i64 = 12_800;
const A_MILLI: std::ops::RangeInclusive<i64> = 500..=1500;
const B_MAX: i64 = 6_400;

/// A shallow-depth rule and the least depth at which it gives `1 <= s < d`
/// with `d - s` even.
type Rule = (&'static str, fn(u32) -> u32, u32);

const RULES: [Rule; 3] = [
    ("v1", |d| 2 * (d / 4) + (d & 1), 3),
    ("d-4", |d| d - 4, 5),
    ("d-6", |d| d - 6, 7),
];

/// Pairs `(v_s, v_d)` and the positions' empties, per fold.
#[derive(Default, Clone)]
struct Cell {
    pairs: [Vec<(i32, i32)>; 3],
    empties: [Vec<u32>; 3],
}

type Grid = Vec<Vec<Cell>>;

/// `grid[d][bucket]` for depths from the rule's minimum, at most one position
/// per game in each cell (samples come sorted by unit id: the lowest is kept).
fn cells(samples: &[(usize, Sample)], (_, rule, least): Rule) -> Grid {
    let mut grid = vec![vec![Cell::default(); BUCKETS.len()]; *DEPTHS.end() as usize + 1];
    let mut used = HashSet::new();
    for (fold, sample) in samples {
        let Some(bucket) = BUCKETS
            .iter()
            .position(|&(lo, hi)| (lo..=hi).contains(&sample.empties))
        else {
            continue;
        };
        for d in least.max(*DEPTHS.start())..=*DEPTHS.end() {
            if let (Some(&vd), Some(&vs)) = (
                sample.values.get(d as usize),
                sample.values.get(rule(d) as usize),
            ) && used.insert((d, bucket, sample.game))
            {
                let cell = &mut grid[d as usize][bucket];
                cell.pairs[*fold].push((vs, vd));
                cell.empties[*fold].push(sample.empties);
            }
        }
    }
    grid
}

fn floor_div(n: i64, d: i64) -> i64 {
    n.div_euclid(d)
}

fn ceil_div(n: i64, d: i64) -> i64 {
    -(-n).div_euclid(d)
}

/// The model exactly as the engine evaluates it: slope in thousandths,
/// intercept in centidiscs, σ coefficients in hundredths of a centidisc.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Runtime {
    a_milli: i64,
    b: i64,
    s: [i64; 3],
}

impl Runtime {
    fn sigma(&self, e: u32, d: u32) -> i64 {
        let raw = self.s[0] + self.s[1] * i64::from(e) + self.s[2] * i64::from(d);
        ceil_div(raw, 100).clamp(1, SIGMA_MAX)
    }

    /// Whether one pair is a proxy boundary error (high, low) at `t`.
    fn error(&self, (vs, vd): (i32, i32), e: u32, d: u32, t: i64) -> (bool, bool) {
        let m = ceil_div(t * self.sigma(e, d), 100);
        let (scaled, vd) = (self.a_milli * i64::from(vs), i64::from(vd));
        (
            vd < floor_div(scaled, 1000) + self.b - m,
            vd > ceil_div(scaled, 1000) + self.b + m,
        )
    }

    /// Root-mean-square residual of `v_d − (a·v_s + b)`.
    fn residual(&self, pairs: &[(i32, i32)]) -> f64 {
        let a = self.a_milli as f64 / 1000.0;
        let sum: f64 = pairs
            .iter()
            .map(|&(vs, vd)| (f64::from(vd) - a * f64::from(vs) - self.b as f64).powi(2))
            .sum();
        (sum / pairs.len().max(1) as f64).sqrt()
    }

    /// Least σ over the empties range a cell's positions span (σ is monotone
    /// in `e`, so the ends suffice).
    fn least(&self, empties: &[u32], d: u32) -> i64 {
        let (lo, hi) = (empties.iter().min(), empties.iter().max());
        match (lo, hi) {
            (Some(&lo), Some(&hi)) => self.sigma(lo, d).min(self.sigma(hi, d)),
            _ => 1,
        }
    }

    /// The cell meets the σ floor on `fold`: it has positions, and its least
    /// runtime σ is at least `FLOOR` times their residual spread.
    fn floor_met(&self, cell: &Cell, fold: usize, d: u32) -> bool {
        let pairs = &cell.pairs[fold];
        !pairs.is_empty()
            && self.least(&cell.empties[fold], d) as f64 >= FLOOR * self.residual(pairs)
    }

    /// Proxy errors (high, low) over a cell's pairs on `fold`.
    fn errors(&self, cell: &Cell, fold: usize, d: u32) -> (usize, usize) {
        cell.pairs[fold]
            .iter()
            .zip(&cell.empties[fold])
            .map(|(&pair, &e)| self.error(pair, e, d, CHECK_T))
            .fold((0, 0), |(h, l), (high, low)| {
                (h + usize::from(high), l + usize::from(low))
            })
    }

    /// Selection acceptance: enough positions, the floor, and proxy error
    /// upper bounds within `MAX_ERROR`.
    fn accepts(&self, cell: &Cell, fold: usize, d: u32) -> bool {
        let n = cell.pairs[fold].len();
        let (high, low) = self.errors(cell, fold, d);
        n >= MIN_POSITIONS
            && self.floor_met(cell, fold, d)
            && wilson_upper(high, n) <= MAX_ERROR
            && wilson_upper(low, n) <= MAX_ERROR
    }
}

/// Least squares `v_d = a·v_s + b`; returns (a, b).
fn regress(pairs: &[(f64, f64)]) -> (f64, f64) {
    let n = pairs.len() as f64;
    let (sx, sy) = pairs
        .iter()
        .fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
    let (mx, my) = (sx / n, sy / n);
    let (sxy, sxx) = pairs.iter().fold((0.0, 0.0), |(xy, xx), p| {
        (xy + (p.0 - mx) * (p.1 - my), xx + (p.0 - mx).powi(2))
    });
    let a = if sxx > 0.0 { sxy / sxx } else { 1.0 };
    (a, my - a * mx)
}

fn midpoint(bucket: usize) -> f64 {
    f64::from(BUCKETS[bucket].0 + BUCKETS[bucket].1) / 2.0
}

/// Weighted least squares of `y` on the regressors; `None` when the system is
/// underidentified (fewer rows than coefficients, or singular).
fn least_squares(rows: &[(Vec<f64>, f64, f64)]) -> Option<Vec<f64>> {
    let k = rows.first()?.0.len();
    if rows.len() < k {
        return None;
    }
    let mut m = vec![vec![0.0; k + 1]; k];
    for (x, y, w) in rows {
        for i in 0..k {
            for j in 0..k {
                m[i][j] += w * x[i] * x[j];
            }
            m[i][k] += w * x[i] * y;
        }
    }
    let scale = (0..k).map(|i| m[i][i].abs()).fold(0.0, f64::max);
    for col in 0..k {
        let pivot = (col..k).max_by(|&p, &q| m[p][col].abs().total_cmp(&m[q][col].abs()))?;
        m.swap(col, pivot);
        if m[col][col].abs() <= 1e-9 * scale {
            return None;
        }
        for row in 0..k {
            if row != col {
                let factor = m[row][col] / m[col][col];
                let source = m[col].clone();
                for (target, value) in m[row].iter_mut().zip(source).skip(col) {
                    *target -= factor * value;
                }
            }
        }
    }
    Some((0..k).map(|i| m[i][k] / m[i][i]).collect())
}

/// Upper end of the 95% Wilson interval for `hits` of `n`.
fn wilson_upper(hits: usize, n: usize) -> f64 {
    if n == 0 {
        return 1.0;
    }
    let (p, n, z) = (hits as f64 / n as f64, n as f64, 1.96f64);
    (p + z * z / (2.0 * n) + z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt())
        / (1.0 + z * z / n)
}

/// A fitted form before quantization: line (a, b), σ = c0 + c1·e + c2·d.
#[derive(Clone, Copy, Debug)]
struct Fitted {
    line: (f64, f64),
    c: [f64; 3],
}

/// The runtime representation of `fitted` with σ scaled by `k`, or `None`
/// when it is not representable (slope outside 500–1500 thousandths,
/// intercept beyond ±6400, a coefficient outside `i32`).
fn quantize(fitted: &Fitted, k: f64) -> Option<Runtime> {
    let a_milli = (fitted.line.0 * 1000.0).round();
    let b = fitted.line.1.round();
    if !a_milli.is_finite()
        || !A_MILLI.contains(&(a_milli as i64))
        || !b.is_finite()
        || b.abs() > B_MAX as f64
    {
        return None;
    }
    let mut s = [0i64; 3];
    for (slot, c) in s.iter_mut().zip(fitted.c) {
        let value = (c * k * 100.0).round();
        if !value.is_finite() || value.abs() > f64::from(i32::MAX) {
            return None;
        }
        *slot = value as i64;
    }
    Some(Runtime {
        a_milli: a_milli as i64,
        b: b as i64,
        s,
    })
}

/// The forms fitted on the fit fold over every cell with enough positions;
/// an underidentified form is left out.
fn forms(grid: &Grid) -> Vec<(&'static str, Fitted)> {
    let cells: Vec<(u32, usize)> = DEPTHS
        .flat_map(|d| (0..BUCKETS.len()).map(move |b| (d, b)))
        .filter(|&(d, b)| grid[d as usize][b].pairs[0].len() >= MIN_POSITIONS)
        .collect();
    let float = |pairs: &[(i32, i32)]| -> Vec<(f64, f64)> {
        pairs
            .iter()
            .map(|&(x, y)| (f64::from(x), f64::from(y)))
            .collect()
    };
    let pooled: Vec<(f64, f64)> = cells
        .iter()
        .flat_map(|&(d, b)| float(&grid[d as usize][b].pairs[0]))
        .collect();
    let linear = |line: (f64, f64)| -> Option<Fitted> {
        let rows: Vec<(Vec<f64>, f64, f64)> = cells
            .iter()
            .map(|&(d, b)| {
                let pairs = float(&grid[d as usize][b].pairs[0]);
                let sum: f64 = pairs
                    .iter()
                    .map(|p| (p.1 - line.0 * p.0 - line.1).powi(2))
                    .sum();
                let spread = (sum / pairs.len() as f64).sqrt();
                (
                    vec![1.0, midpoint(b), f64::from(d)],
                    spread,
                    pairs.len() as f64,
                )
            })
            .collect();
        let c = least_squares(&rows)?;
        Some(Fitted {
            line,
            c: [c[0], c[1], c[2]],
        })
    };
    let mut out = Vec::new();
    if let Some(fitted) = linear((1.0, 0.0)) {
        out.push(("a=1 b=0", fitted));
    }
    if !pooled.is_empty()
        && let Some(fitted) = linear(regress(&pooled))
    {
        out.push(("pooled a b", fitted));
    }
    out
}

/// Depth range and bucket range, inclusive.
type Rect = ((u32, u32), (usize, usize));

fn rect_cells(((d0, d1), (b0, b1)): Rect) -> impl Iterator<Item = (u32, usize)> {
    (d0..=d1).flat_map(move |d| (b0..=b1).map(move |b| (d, b)))
}

/// The rectangle order: most cells, smallest mean σ, smallest first depth,
/// largest last depth, smallest first bucket, largest last bucket.
fn better(x: (usize, f64, Rect), y: (usize, f64, Rect)) -> bool {
    let (((xd0, xd1), (xb0, xb1)), ((yd0, yd1), (yb0, yb1))) = (x.2, y.2);
    y.0.cmp(&x.0)
        .then(x.1.total_cmp(&y.1))
        .then(xd0.cmp(&yd0))
        .then(yd1.cmp(&xd1))
        .then(xb0.cmp(&yb0))
        .then(yb1.cmp(&xb1))
        .is_lt()
}

/// Mean runtime σ over the selection positions of a rectangle.
fn mean_sigma(grid: &Grid, model: &Runtime, rect: Rect) -> f64 {
    let (mut sum, mut n) = (0.0, 0usize);
    for (d, b) in rect_cells(rect) {
        let cell = &grid[d as usize][b];
        for &e in &cell.empties[1] {
            sum += model.sigma(e, d) as f64;
            n += 1;
        }
    }
    sum / n.max(1) as f64
}

/// The quantized model with σ inflated just enough that every cell of the
/// rectangle meets the floor on `fold`, or `None` if it is unrepresentable.
fn inflate(grid: &Grid, fold: usize, fitted: &Fitted, rect: Rect) -> Option<Runtime> {
    let mut k = 1.0f64;
    for _ in 0..4000 {
        let model = quantize(fitted, k)?;
        if rect_cells(rect).all(|(d, b)| model.floor_met(&grid[d as usize][b], fold, d)) {
            return Some(model);
        }
        k *= 1.005;
    }
    None
}

/// The best rectangle within `allowed` whose every cell the inflated,
/// quantized model accepts on the selection fold.
fn best_rect(
    grid: &Grid,
    fitted: &Fitted,
    least: u32,
    allowed: &dyn Fn(u32, usize) -> bool,
) -> Option<(Rect, Runtime)> {
    let mut best: Option<(usize, f64, Rect, Runtime)> = None;
    let depths = least.max(*DEPTHS.start())..=*DEPTHS.end();
    for d0 in depths.clone() {
        for d1 in d0..=*depths.end() {
            for b0 in 0..BUCKETS.len() {
                for b1 in b0..BUCKETS.len() {
                    let rect = ((d0, d1), (b0, b1));
                    if rect_cells(rect).any(|(d, b)| {
                        !allowed(d, b) || grid[d as usize][b].pairs[1].len() < MIN_POSITIONS
                    }) {
                        continue;
                    }
                    let Some(model) = inflate(grid, 1, fitted, rect) else {
                        continue;
                    };
                    if !rect_cells(rect).all(|(d, b)| model.accepts(&grid[d as usize][b], 1, d)) {
                        continue;
                    }
                    let (count, mean) = (rect_cells(rect).count(), mean_sigma(grid, &model, rect));
                    if best
                        .as_ref()
                        .is_none_or(|&(c, m, r, _)| better((count, mean, rect), (c, m, r)))
                    {
                        best = Some((count, mean, rect, model));
                    }
                }
            }
        }
    }
    best.map(|(_, _, rect, model)| (rect, model))
}

/// Empties bounds of a rectangle, intersected with the selection positions
/// in it, the solver threshold and the largest supported count.
fn empties_bounds(grid: &Grid, ((d0, d1), (b0, b1)): Rect) -> (u32, u32) {
    let seen: Vec<u32> = rect_cells(((d0, d1), (b0, b1)))
        .flat_map(|(d, b)| grid[d as usize][b].empties[1].iter().copied())
        .collect();
    let low = seen.iter().copied().min().unwrap_or(BUCKETS[b0].0);
    let high = seen.iter().copied().max().unwrap_or(BUCKETS[b1].1);
    (
        BUCKETS[b0].0.max(low).max(THRESHOLD + 2),
        BUCKETS[b1].1.min(high).min(MAX_EMPTIES),
    )
}

/// The selected rule, form, rectangle and quantized model.
struct Choice {
    rule: Rule,
    form: &'static str,
    rect: Rect,
    model: Runtime,
}

/// Chooses from the fit and selection folds only (audit pairs never enter).
fn choose(samples: &[(usize, Sample)]) -> Result<Choice, String> {
    let allowed = |_: u32, b: usize| BUCKETS[b].1 >= THRESHOLD + 2 && BUCKETS[b].0 <= MAX_EMPTIES;
    let mut chosen: Option<(usize, f64, Choice)> = None;
    for rule in RULES {
        let grid = cells(samples, rule);
        let fitted = forms(&grid);
        if fitted.is_empty() {
            println!("rule {}: no identifiable form", rule.0);
        }
        for (form, fitted) in fitted {
            if quantize(&fitted, 1.0).is_none() {
                println!(
                    "rule {} form {form}: not representable (a {:.4} b {:.1})",
                    rule.0, fitted.line.0, fitted.line.1
                );
                continue;
            }
            match best_rect(&grid, &fitted, rule.2, &allowed) {
                Some((rect, model)) => {
                    let (count, mean) = (rect_cells(rect).count(), mean_sigma(&grid, &model, rect));
                    println!(
                        "rule {} form {form}: rectangle depths {}..={} buckets {}..={} cells {count} mean_sigma {mean:.1} model {model:?}",
                        rule.0, rect.0.0, rect.0.1, BUCKETS[rect.1.0].0, BUCKETS[rect.1.1].1
                    );
                    if chosen
                        .as_ref()
                        .is_none_or(|c| better((count, mean, rect), (c.0, c.1, c.2.rect)))
                    {
                        chosen = Some((
                            count,
                            mean,
                            Choice {
                                rule,
                                form,
                                rect,
                                model,
                            },
                        ));
                    }
                }
                None => println!("rule {} form {form}: no accepted rectangle", rule.0),
            }
        }
    }
    chosen
        .map(|c| c.2)
        .ok_or_else(|| "no rule and form has an accepted rectangle".into())
}

/// The `ProbCut` fields of a model and rectangle (`t` is set by tuning).
fn emit(model: &Runtime, (d0, d1): (u32, u32), (e_min, e_max): (u32, u32)) -> String {
    format!(
        "ProbCut {{ t: T, s0: {}, s1: {}, s2: {}, a_milli: {}, b: {}, d_min: {d0}, d_max: {d1}, e_min: {e_min}, e_max: {e_max} }}",
        model.s[0], model.s[1], model.s[2], model.a_milli, model.b
    )
}

/// The model fields of an emitted `ProbCut` line.
#[cfg(test)]
fn parse_emitted(line: &str) -> Option<Runtime> {
    let get = |name: &str| -> Option<i64> {
        let at = line.find(&format!(" {name}: "))? + name.len() + 3;
        line[at..].split([',', ' ']).next()?.parse().ok()
    };
    Some(Runtime {
        a_milli: get("a_milli")?,
        b: get("b")?,
        s: [get("s0")?, get("s1")?, get("s2")?],
    })
}

/// Audits the frozen choice on the audit fold. A cell with fewer than
/// `MIN_POSITIONS` audit positions is unsupported; it and any cell failing
/// the floor shrink the rectangle by the rectangle order (no refit). Returns
/// the surviving rectangle, or an error if none survives.
fn audit(grid: &Grid, choice: &Choice) -> Result<Rect, String> {
    let model = &choice.model;
    let mut failed = Vec::new();
    for (d, b) in rect_cells(choice.rect) {
        let cell = &grid[d as usize][b];
        let n = cell.pairs[2].len();
        let (high, low) = model.errors(cell, 2, d);
        let verdict = if n < MIN_POSITIONS {
            "UNSUPPORTED"
        } else if model.floor_met(cell, 2, d) {
            "ok"
        } else {
            "FAIL"
        };
        println!(
            "  d {d:2} bucket {:2}-{:2} n {n:4} least_sigma {:5} audit_sd {:6.1} floor {verdict} high {high} (up {:.1}%) low {low} (up {:.1}%)",
            BUCKETS[b].0,
            BUCKETS[b].1,
            model.least(&cell.empties[2], d),
            model.residual(&cell.pairs[2]),
            100.0 * wilson_upper(high, n),
            100.0 * wilson_upper(low, n)
        );
        if verdict != "ok" {
            failed.push((d, b));
        }
    }
    if failed.is_empty() {
        return Ok(choice.rect);
    }
    let ((d0, d1), (b0, b1)) = choice.rect;
    let mut best: Option<(usize, f64, Rect)> = None;
    for e0 in d0..=d1 {
        for e1 in e0..=d1 {
            for c0 in b0..=b1 {
                for c1 in c0..=b1 {
                    let sub = ((e0, e1), (c0, c1));
                    if rect_cells(sub).any(|cell| failed.contains(&cell)) {
                        continue;
                    }
                    let (count, mean) = (rect_cells(sub).count(), mean_sigma(grid, model, sub));
                    if best.is_none_or(|b| better((count, mean, sub), b)) {
                        best = Some((count, mean, sub));
                    }
                }
            }
        }
    }
    println!(
        "audit: {} cells unsupported or failing the floor",
        failed.len()
    );
    best.map(|(_, _, sub)| sub)
        .ok_or_else(|| "audit: no rectangle survives".into())
}

fn report_cells(name: &str, grid: &Grid, model: Option<&Runtime>, rule: fn(u32) -> u32) {
    println!(
        "\nrule {name}: per cell; spread of v_d - v_s on fit and selection (audit counted only){}",
        if model.is_some() {
            "; least selection sigma"
        } else {
            ""
        }
    );
    println!(
        "   d  s  bucket  n_fit n_sel n_aud  sd_fit  sd_sel{}",
        if model.is_some() { "  sigma" } else { "" }
    );
    let unit = Runtime {
        a_milli: 1000,
        b: 0,
        s: [0; 3],
    };
    for d in DEPTHS {
        for (bucket, cell) in grid[d as usize].iter().enumerate() {
            let [f, s, a] = &cell.pairs;
            if f.is_empty() {
                continue;
            }
            print!(
                "  {d:2} {:2}  {:2}-{:2}  {:5} {:5} {:5}  {:6.1}  {:6.1}",
                rule(d),
                BUCKETS[bucket].0,
                BUCKETS[bucket].1,
                f.len(),
                s.len(),
                a.len(),
                unit.residual(f),
                unit.residual(s)
            );
            match model {
                Some(model) => println!("  {:5}", model.least(&cell.empties[1], d)),
                None => println!(),
            }
        }
    }
}

fn fit(args: &[String]) -> Result<(), String> {
    let usage = "usage: probcut_fit fit --in FILE --dev FILE [--audit]";
    let map = options(args, &["--in", "--dev"], &["--audit"])
        .map_err(|error| format!("{error}\n{usage}"))?;
    let (Some(input), Some(dev_path)) = (map.get("--in"), map.get("--dev")) else {
        return Err(usage.into());
    };
    let development: HashSet<usize> = std::fs::read_to_string(dev_path)
        .map_err(|error| format!("--dev {dev_path}: {error}"))?
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            line.trim()
                .parse()
                .map_err(|_| format!("bad unit id {line}"))
        })
        .collect::<Result<_, _>>()?;
    let raw = samples(Path::new(input))?;
    let total = raw.len();
    let samples = distinct(raw, &development);
    let counts = [0, 1, 2].map(|f| samples.iter().filter(|(fold, _)| *fold == f).count());
    println!(
        "probcut_fit fit {input} records {total} development {} kept {} fit {} selection {} audit {} min_positions {MIN_POSITIONS} floor {FLOOR} max_error {MAX_ERROR} check_t {CHECK_T}",
        development.len(),
        samples.len(),
        counts[0],
        counts[1],
        counts[2]
    );
    for rule in RULES {
        report_cells(rule.0, &cells(&samples, rule), None, rule.1);
    }
    println!();
    let choice = choose(&samples)?;
    let grid = cells(&samples, choice.rule);
    report_cells(choice.rule.0, &grid, Some(&choice.model), choice.rule.1);
    let bounds = empties_bounds(&grid, choice.rect);
    println!(
        "\ncandidate (selection fold, not audited): rule {} form {} rectangle depths {}..={} empties {}..={} model {:?}",
        choice.rule.0,
        choice.form,
        choice.rect.0.0,
        choice.rect.0.1,
        bounds.0,
        bounds.1,
        choice.model
    );
    if !map.contains_key("--audit") {
        println!("audit fold not opened (pass --audit once the choice is frozen)");
        return Ok(());
    }
    println!("\naudit fold, frozen model, per cell:");
    let rect = audit(&grid, &choice)?;
    println!(
        "audited {}",
        emit(&choice.model, rect.0, empties_bounds(&grid, rect))
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("collect") => collect(&args[1..]),
        Some("fit") => fit(&args[1..]),
        _ => Err(
            "usage: probcut_fit collect --out FILE ... | fit --in FILE --dev FILE [--audit]".into(),
        ),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic integer pairs `v_d = a·v_s + b + e`, e of spread `sigma`.
    fn synthetic(a: f64, b: f64, sigma: f64, n: usize, seed: u64) -> Vec<(i32, i32)> {
        let mut rng = Rng(seed);
        (0..n)
            .map(|_| {
                let x = rng.below(4001) as f64 - 2000.0;
                // Sum of 12 uniforms in [0, 1) minus 6: mean 0, variance 1.
                let z: f64 = (0..12)
                    .map(|_| rng.below(1 << 20) as f64 / f64::from(1 << 20))
                    .sum::<f64>()
                    - 6.0;
                (x as i32, (a * x + b + sigma * z).round() as i32)
            })
            .collect()
    }

    fn float(pairs: &[(i32, i32)]) -> Vec<(f64, f64)> {
        pairs
            .iter()
            .map(|&(x, y)| (f64::from(x), f64::from(y)))
            .collect()
    }

    /// A grid of depths 3–8 and buckets 1–4 with spread 300 on every fold,
    /// except `noisy` cells, whose selection and audit pairs have spread 900.
    fn grid_with(noisy: &[(u32, usize)], audit: usize) -> Grid {
        let mut grid = vec![vec![Cell::default(); BUCKETS.len()]; *DEPTHS.end() as usize + 1];
        for d in 3..=8u32 {
            for b in 1..BUCKETS.len() {
                for fold in 0..3 {
                    let sigma = if fold > 0 && noisy.contains(&(d, b)) {
                        900.0
                    } else {
                        300.0
                    };
                    let n = if fold == 2 { audit } else { 200 };
                    let seed = u64::from(d) * 10 + b as u64 + 100 * fold as u64;
                    let cell = &mut grid[d as usize][b];
                    cell.pairs[fold] = synthetic(1.0, 0.0, sigma, n, seed);
                    cell.empties[fold] = (0..n).map(|i| BUCKETS[b].0 + (i % 8) as u32).collect();
                }
            }
        }
        grid
    }

    const FLAT: Fitted = Fitted {
        line: (1.0, 0.0),
        c: [300.0, 0.0, 0.0],
    };

    #[test]
    fn regression_recovers_line() {
        let pairs = float(&synthetic(0.9, 50.0, 300.0, 20_000, 1));
        let (a, b) = regress(&pairs);
        assert!((a - 0.9).abs() < 0.01, "a {a}");
        assert!((b - 50.0).abs() < 10.0, "b {b}");
    }

    #[test]
    fn least_squares_identifies() {
        let rows: Vec<(Vec<f64>, f64, f64)> = (0..20)
            .map(|i| {
                let (e, d) = (f64::from(i % 5) * 8.0 + 16.0, f64::from(i / 5) + 3.0);
                (
                    vec![1.0, e, d],
                    200.0 + 3.0 * e - 7.0 * d,
                    1.0 + f64::from(i),
                )
            })
            .collect();
        let c = least_squares(&rows).expect("identified");
        for (got, want) in c.iter().zip([200.0, 3.0, -7.0]) {
            assert!((got - want).abs() < 1e-6, "{c:?}");
        }
        // One bucket only: e is collinear with the intercept.
        let single: Vec<(Vec<f64>, f64, f64)> = (0..6)
            .map(|i| (vec![1.0, 27.5, f64::from(i) + 3.0], 300.0, 1.0))
            .collect();
        assert!(least_squares(&single).is_none());
        assert!(least_squares(&rows[..2]).is_none());
    }

    #[test]
    fn wilson_bounds() {
        assert!((wilson_upper(0, 100) - 0.037).abs() < 0.001);
        assert!(wilson_upper(5, 100) > 0.05 && wilson_upper(5, 100) < 0.12);
        assert_eq!(wilson_upper(0, 0), 1.0);
    }

    #[test]
    fn shallow_rules_valid() {
        for (name, rule, least) in RULES {
            for d in least..=*DEPTHS.end() {
                let s = rule(d);
                assert!(s >= 1 && s < d && (d - s) % 2 == 0, "{name} d {d} s {s}");
            }
        }
    }

    #[test]
    fn canonical_symmetry() {
        let board = Board {
            me: 0x0000_0010_3800_0000,
            opp: 0x0000_0808_0400_0200,
        };
        let key = canonical(board);
        let image = |f: fn(u64) -> u64| Board {
            me: f(board.me),
            opp: f(board.opp),
        };
        for f in [flip_vertical, mirror_horizontal, transpose, |x| {
            transpose(flip_vertical(x))
        }] {
            assert_eq!(canonical(image(f)), key);
        }
        assert_eq!(transpose(transpose(board.me)), board.me);
        assert_eq!(transpose(1 << 1), 1 << 8);
        assert_ne!(
            canonical(Board {
                me: board.opp,
                opp: board.me
            }),
            key
        );
    }

    #[test]
    fn distinct_keeps_lowest() {
        let board = Board {
            me: 0x0000_0010_3800_0000,
            opp: 0x0000_0808_0400_0200,
        };
        let mirrored = Board {
            me: mirror_horizontal(board.me),
            opp: mirror_horizontal(board.opp),
        };
        let sample = |id, board| Sample {
            id,
            game: id,
            board,
            empties: 50,
            values: vec![0],
        };
        let kept = distinct(
            vec![
                sample(7, mirrored),
                sample(3, board),
                sample(5, Board::START),
            ],
            &HashSet::new(),
        );
        let ids: Vec<usize> = kept.iter().map(|(_, s)| s.id).collect();
        assert_eq!(ids, [3, 5]);
        assert_eq!(kept[0].0, fold(3));
    }

    #[test]
    fn development_never_audit() {
        let audit_game = (0..1000).find(|&g| fold(g) == 2).unwrap();
        let other = (0..1000).find(|&g| fold(g) == 0).unwrap();
        let board = |n: u64| Board {
            me: 1 << n,
            opp: 1 << (n + 20),
        };
        let samples = vec![
            Sample {
                id: 1,
                game: audit_game,
                board: board(1),
                empties: 50,
                values: vec![0],
            },
            Sample {
                id: 2,
                game: audit_game,
                board: board(2),
                empties: 50,
                values: vec![0],
            },
            Sample {
                id: 3,
                game: other,
                board: board(3),
                empties: 50,
                values: vec![0],
            },
        ];
        let kept: Vec<(usize, usize)> = distinct(samples, &HashSet::from([1, 3]))
            .iter()
            .map(|(f, s)| (*f, s.id))
            .collect();
        assert_eq!(kept, [(2, 2), (0, 3)]);
    }

    #[test]
    fn one_per_game() {
        let board = |n: u64| Board {
            me: 1 << n,
            opp: 1 << (n + 20),
        };
        let values: Vec<i32> = (0..=4).collect();
        let samples = vec![
            (
                0,
                Sample {
                    id: 1,
                    game: 9,
                    board: board(1),
                    empties: 30,
                    values: values.clone(),
                },
            ),
            (
                0,
                Sample {
                    id: 2,
                    game: 9,
                    board: board(2),
                    empties: 31,
                    values: values.clone(),
                },
            ),
            (
                0,
                Sample {
                    id: 3,
                    game: 8,
                    board: board(3),
                    empties: 30,
                    values,
                },
            ),
        ];
        let grid = cells(&samples, RULES[0]);
        assert_eq!(grid[3][1].pairs[0].len(), 2);
        assert_eq!(grid[3][1].empties[0], [30, 30]);
    }

    #[test]
    fn rectangle_order_bounds() {
        let r = |d0, d1, b0, b1| ((d0, d1), (b0, b1));
        assert!(better((6, 300.0, r(3, 5, 1, 2)), (4, 100.0, r(3, 4, 1, 2))));
        assert!(better((6, 299.0, r(4, 6, 1, 2)), (6, 300.0, r(3, 5, 1, 2))));
        assert!(better((6, 300.0, r(3, 5, 1, 2)), (6, 300.0, r(4, 6, 1, 2))));
        assert!(better((6, 300.0, r(3, 5, 1, 2)), (6, 300.0, r(3, 5, 2, 3))));
        assert!(!better(
            (6, 300.0, r(3, 5, 1, 2)),
            (6, 300.0, r(3, 5, 1, 2))
        ));
        let mut grid = vec![vec![Cell::default(); BUCKETS.len()]; *DEPTHS.end() as usize + 1];
        grid[3][0].empties[1] = vec![21, 23];
        grid[3][1].empties[1] = vec![24, 29];
        assert_eq!(empties_bounds(&grid, r(3, 3, 0, 1)), (21, 29));
        grid[3][0].empties[1] = vec![17];
        assert_eq!(empties_bounds(&grid, r(3, 3, 0, 1)), (THRESHOLD + 2, 29));
    }

    #[test]
    fn rectangle_excludes_noise() {
        let grid = grid_with(&[(6, 4)], 200);
        let (rect, model) = best_rect(&grid, &FLAT, 3, &|_, _| true).expect("a rectangle");
        assert!(rect_cells(rect).all(|cell| cell != (6, 4)), "{rect:?}");
        assert_eq!(rect_cells(rect).count(), 18, "{rect:?}");
        assert!(model.s[0] >= 30_000 && model.s[0] < 33_000, "{model:?}");
    }

    #[test]
    fn runtime_arithmetic() {
        let model = Runtime {
            a_milli: 1000,
            b: 0,
            s: [30_000, 0, 0],
        };
        assert_eq!(model.sigma(30, 5), 300);
        // m = ceil(1.5 · 300) = 450: v_s − 450 is the high boundary.
        assert_eq!(model.error((0, -450), 30, 5, 150), (false, false));
        assert_eq!(model.error((0, -451), 30, 5, 150), (true, false));
        assert_eq!(model.error((0, 451), 30, 5, 150), (false, true));
        // Signed rounding: floor(-1.5) = -2 for high, ceil(-1.5) = -1 for low.
        let half = Runtime {
            a_milli: 1500,
            ..model
        };
        assert_eq!(half.error((-1, -452), 30, 5, 150), (false, false));
        assert_eq!(half.error((-1, -453), 30, 5, 150), (true, false));
        assert_eq!(half.error((-1, 449), 30, 5, 150), (false, false));
        assert_eq!(half.error((-1, 450), 30, 5, 150), (false, true));
        assert_eq!(
            Runtime {
                s: [-5, 0, 0],
                ..model
            }
            .sigma(30, 5),
            1
        );
        assert_eq!(
            Runtime {
                s: [i64::from(i32::MAX), 0, 0],
                ..model
            }
            .sigma(30, 5),
            SIGMA_MAX
        );
        assert_eq!(
            Runtime {
                s: [101, 0, 0],
                ..model
            }
            .sigma(30, 5),
            2
        );
    }

    #[test]
    fn slope_two_unrepresentable() {
        let mut grid = vec![vec![Cell::default(); BUCKETS.len()]; *DEPTHS.end() as usize + 1];
        for d in 3..=6u32 {
            for b in 1..4 {
                let pairs: Vec<(i32, i32)> = (0..80)
                    .map(|i| (i * 10 - 400, 2 * (i * 10 - 400) + (i % 7) * 50))
                    .collect();
                grid[d as usize][b].pairs[0] = pairs;
                grid[d as usize][b].empties[0] = vec![BUCKETS[b].0; 80];
            }
        }
        let forms = forms(&grid);
        let pooled = forms
            .iter()
            .find(|(name, _)| *name == "pooled a b")
            .expect("pooled form fitted");
        assert!((pooled.1.line.0 - 2.0).abs() < 0.05);
        assert_eq!(quantize(&pooled.1, 1.0), None);
        assert!(
            quantize(
                &Fitted {
                    line: (1.5, 0.0),
                    ..FLAT
                },
                1.0
            )
            .is_some()
        );
        assert_eq!(
            quantize(
                &Fitted {
                    line: (1.0, 6400.6),
                    ..FLAT
                },
                1.0
            ),
            None
        );
    }

    #[test]
    fn bucket_edge_floor() {
        // σ falls with e: 600 at e = 24, 480 at e = 28, 390 at e = 31.
        let model = Runtime {
            a_milli: 1000,
            b: 0,
            s: [132_000, -3_000, 0],
        };
        let pairs = synthetic(1.0, 0.0, 440.0, 400, 7);
        let mut cell = Cell::default();
        cell.pairs[1] = pairs;
        cell.empties[1] = (0..400).map(|i| 24 + (i % 8) as u32).collect();
        let rms = model.residual(&cell.pairs[1]);
        assert!(
            model.sigma(28, 4) as f64 >= FLOOR * rms,
            "midpoint would pass"
        );
        assert!(!model.floor_met(&cell, 1, 4), "edge e = 31 fails");
        cell.empties[1] = vec![24; 400];
        assert!(model.floor_met(&cell, 1, 4));
    }

    #[test]
    fn emitted_round_trip() {
        let grid = grid_with(&[(6, 4)], 200);
        let (rect, model) = best_rect(&grid, &FLAT, 3, &|_, _| true).unwrap();
        let line = emit(&model, rect.0, empties_bounds(&grid, rect));
        let parsed = parse_emitted(&line).expect("parses");
        assert_eq!(parsed, model, "{line}");
        for d in 3..=8u32 {
            for b in 1..BUCKETS.len() {
                let cell = &grid[d as usize][b];
                assert_eq!(parsed.accepts(cell, 1, d), model.accepts(cell, 1, d));
            }
        }
    }

    #[test]
    fn audit_needs_evidence() {
        let empty = grid_with(&[], 0);
        let (rect, model) = best_rect(&empty, &FLAT, 3, &|_, _| true).unwrap();
        let choice = Choice {
            rule: RULES[0],
            form: "a=1 b=0",
            rect,
            model,
        };
        assert!(
            audit(&empty, &choice).is_err(),
            "an empty audit cannot pass"
        );
        let thin = grid_with(&[], 30);
        assert!(audit(&thin, &choice).is_err(), "a thin audit cannot pass");
        let full = grid_with(&[], 200);
        assert_eq!(audit(&full, &choice), Ok(rect));
    }

    #[test]
    fn audit_failure_shrinks() {
        // Selection is clean everywhere; the audit fold is noisy at depth 8, bucket 4.
        let mut grid = grid_with(&[], 200);
        let (rect, model) = best_rect(&grid, &FLAT, 3, &|_, _| true).unwrap();
        assert_eq!(rect_cells(rect).count(), 24);
        grid[8][4].pairs[2] = synthetic(1.0, 0.0, 900.0, 200, 99);
        let choice = Choice {
            rule: RULES[0],
            form: "a=1 b=0",
            rect,
            model,
        };
        let survived = audit(&grid, &choice).expect("a sub-rectangle survives");
        assert!(rect_cells(survived).all(|cell| cell != (8, 4)));
        assert_eq!(rect_cells(survived).count(), 20, "{survived:?}");
    }

    #[test]
    fn audit_labels_inert() {
        // Samples whose values drift by depth with spread about 300 per step.
        let make = |perturb: bool| -> Vec<(usize, Sample)> {
            let mut rng = Rng(5);
            let samples: Vec<Sample> = (0..3000)
                .map(|id| {
                    let mut v = rng.below(2001) as i32 - 1000;
                    let mut values = vec![v];
                    for _ in 1..=10 {
                        v += rng.below(601) as i32 - 300;
                        values.push(v);
                    }
                    let empties = 24 + rng.below(24) as u32;
                    if perturb && fold(id) == 2 {
                        values.iter_mut().for_each(|v| *v = -*v * 3 + 777);
                    }
                    let board = Board {
                        me: (id as u64) << 1 | 1 << 40,
                        opp: 1 << 50 | (id as u64) << 20,
                    };
                    Sample {
                        id,
                        game: id,
                        board,
                        empties,
                        values,
                    }
                })
                .collect();
            distinct(samples, &HashSet::new())
        };
        let (a, b) = (choose(&make(false)), choose(&make(true)));
        match (a, b) {
            (Ok(a), Ok(b)) => {
                assert_eq!(
                    (a.rule.0, a.form, a.rect, a.model),
                    (b.rule.0, b.form, b.rect, b.model)
                );
            }
            (Err(a), Err(b)) => assert_eq!(a, b),
            _ => panic!("audit labels changed the outcome"),
        }
    }
}
