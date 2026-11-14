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
//! extends a finished run.
//!
//! `fit`: reads a `collect` journal and, for each shallow-depth rule, depth
//! `d` and empties bucket, the pairs `(v_s, v_d)` of both splits. It fits the
//! σ forms of the design on the fit split, inflates each until it meets the
//! floor (model σ at least 95% of the held-out σ in every supported cell), and
//! reports held-out boundary error rates at `--t` and the constants.
//!
//! usage: probcut_fit collect --out FILE [--nodes 12000] [--depth 14]
//!        [--deep-depth 16] [--deep-every 8] [--jobs 6] [--seed 145]
//!        probcut_fit fit --in FILE [--t 150] [--min-cell 30]

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

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

fn collect(args: &[String]) -> Result<(), String> {
    let value = |flag: &str, default: &str| {
        let at = args.iter().position(|arg| arg == flag);
        at.and_then(|at| args.get(at + 1))
            .map_or(default.to_string(), String::clone)
    };
    let number = |flag: &str, default: &str| {
        value(flag, default)
            .parse::<u64>()
            .map_err(|_| format!("invalid {flag}"))
    };
    let out = value("--out", "");
    if out.is_empty() {
        return Err("usage: probcut_fit collect --out FILE [--nodes N] [--depth D] [--deep-depth D] [--deep-every K] [--jobs J] [--seed S]".into());
    }
    let nodes = number("--nodes", "12000")? as usize;
    let depth = number("--depth", "14")? as u32;
    let deep = number("--deep-depth", "16")? as u32;
    let every = number("--deep-every", "8")?.max(1) as usize;
    let jobs = number("--jobs", "6")?.max(1) as usize;
    let seed = number("--seed", "145")?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let (games, data) = wthor(&root)?;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    // `--nodes` and `--jobs` are left out: they change neither unit.
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
    let (journal, next, finished) = (
        Mutex::new(journal),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    );
    let start = Instant::now();
    eprintln!(
        "probcut_fit collect: {} of {nodes} units to do, {jobs} jobs",
        pending.len()
    );
    std::thread::scope(|scope| -> Result<(), String> {
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
                        let count = finished.fetch_add(1, Relaxed) + 1;
                        if count % 50 == 0 || count == pending.len() {
                            let elapsed = start.elapsed().as_secs_f64();
                            let eta = elapsed / count as f64 * (pending.len() - count) as f64;
                            eprintln!(
                                "progress {count}/{} elapsed {elapsed:.0}s eta {eta:.0}s",
                                pending.len()
                            );
                        }
                    }
                    Ok(())
                })
            })
            .collect();
        workers
            .into_iter()
            .try_for_each(|worker| worker.join().map_err(|_| "worker panicked".to_string())?)
    })?;
    let journal = journal.into_inner().unwrap();
    let records = journal.records();
    let ms: f64 = records
        .iter()
        .filter_map(|record| {
            record
                .line
                .split(" ms ")
                .nth(1)?
                .split(' ')
                .next()?
                .parse::<f64>()
                .ok()
        })
        .sum();
    println!(
        "probcut_fit collect units {} cpu_s {:.0} wall_s {:.0}",
        records.len(),
        ms / 1000.0,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

struct Sample {
    held: bool,
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

fn samples(path: &Path) -> Result<Vec<Sample>, String> {
    let (_, records) = othello_tools::journal::read(path)?;
    records
        .iter()
        .map(|record| {
            let line = &record.line;
            let values = field(line, "values")
                .ok_or("no values")?
                .split(',')
                .map(|value| value.parse().map_err(|_| "bad value".to_string()))
                .collect::<Result<_, _>>()?;
            Ok(Sample {
                held: field(line, "split") == Some("held"),
                empties: field(line, "empties")
                    .and_then(|e| e.parse().ok())
                    .ok_or("bad empties")?,
                values,
            })
        })
        .collect()
}

/// Empties buckets (inclusive) for cells and the per-stage form.
const BUCKETS: [(u32, u32); 5] = [(16, 23), (24, 31), (32, 39), (40, 47), (48, 60)];
const DEPTHS: std::ops::RangeInclusive<u32> = 3..=16;

type Rule = (&'static str, fn(u32) -> u32);

const RULES: [Rule; 3] = [
    ("v1", |d| 2 * (d / 4) + (d & 1)),
    ("d-4", |d| d.saturating_sub(4).max(1)),
    ("d-6", |d| d.saturating_sub(6).max(1)),
];

#[derive(Default, Clone)]
struct Cell {
    /// (v_s, v_d) by split: fit, held.
    pairs: [Vec<(f64, f64)>; 2],
}

fn cells(samples: &[Sample], rule: fn(u32) -> u32) -> Vec<Vec<Cell>> {
    let mut grid = vec![vec![Cell::default(); BUCKETS.len()]; *DEPTHS.end() as usize + 1];
    for sample in samples {
        let Some(bucket) = BUCKETS
            .iter()
            .position(|&(lo, hi)| (lo..=hi).contains(&sample.empties))
        else {
            continue;
        };
        for d in DEPTHS {
            let s = rule(d) as usize;
            if let (Some(&vd), Some(&vs)) = (sample.values.get(d as usize), sample.values.get(s)) {
                grid[d as usize][bucket].pairs[usize::from(sample.held)]
                    .push((f64::from(vs), f64::from(vd)));
            }
        }
    }
    grid
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

/// Root-mean-square residual of `v_d - (a·v_s + b)`.
fn spread(pairs: &[(f64, f64)], (a, b): (f64, f64)) -> f64 {
    let sum: f64 = pairs.iter().map(|p| (p.1 - a * p.0 - b).powi(2)).sum();
    (sum / pairs.len().max(1) as f64).sqrt()
}

fn midpoint(bucket: usize) -> f64 {
    f64::from(BUCKETS[bucket].0 + BUCKETS[bucket].1) / 2.0
}

/// Weighted least squares of `y` on the given regressors (normal equations).
fn least_squares(rows: &[(Vec<f64>, f64, f64)]) -> Vec<f64> {
    let k = rows[0].0.len();
    let mut m = vec![vec![0.0; k + 1]; k];
    for (x, y, w) in rows {
        for i in 0..k {
            for j in 0..k {
                m[i][j] += w * x[i] * x[j];
            }
            m[i][k] += w * x[i] * y;
        }
    }
    for col in 0..k {
        let pivot = (col..k)
            .max_by(|&p, &q| m[p][col].abs().total_cmp(&m[q][col].abs()))
            .unwrap();
        m.swap(col, pivot);
        for row in 0..k {
            if row != col && m[col][col] != 0.0 {
                let factor = m[row][col] / m[col][col];
                let source = m[col].clone();
                for (target, value) in m[row].iter_mut().zip(source).skip(col) {
                    *target -= factor * value;
                }
            }
        }
    }
    (0..k)
        .map(|i| {
            if m[i][i] != 0.0 {
                m[i][k] / m[i][i]
            } else {
                0.0
            }
        })
        .collect()
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

struct Form {
    name: &'static str,
    /// σ(d, bucket) in centidiscs, before inflation.
    sigma: Box<dyn Fn(u32, usize) -> f64>,
    line: (f64, f64),
    constants: Vec<f64>,
}

fn fit(args: &[String]) -> Result<(), String> {
    let value = |flag: &str, default: &str| {
        let at = args.iter().position(|arg| arg == flag);
        at.and_then(|at| args.get(at + 1))
            .map_or(default.to_string(), String::clone)
    };
    let input = value("--in", "");
    if input.is_empty() {
        return Err("usage: probcut_fit fit --in FILE [--t 150] [--min-cell 30]".into());
    }
    let t = value("--t", "150")
        .parse::<f64>()
        .map_err(|_| "invalid --t")?
        / 100.0;
    let min_cell: usize = value("--min-cell", "30")
        .parse()
        .map_err(|_| "invalid --min-cell")?;
    let samples = samples(Path::new(&input))?;
    let held = samples.iter().filter(|s| s.held).count();
    println!(
        "probcut_fit fit {input} samples {} fit {} held {held} t {t:.2} min_cell {min_cell}",
        samples.len(),
        samples.len() - held
    );
    for (name, rule) in RULES {
        let grid = cells(&samples, rule);
        println!(
            "\nrule {name}: per cell, a=1 b=0 spread (fit / held), fitted a b, residual spread (held)"
        );
        println!("   d  s  bucket   n_fit n_held   sd_fit sd_held      a        b  resid_held");
        let mut rows = Vec::new();
        for d in DEPTHS {
            for (bucket, cell) in grid[d as usize].iter().enumerate() {
                let [fit, held] = &cell.pairs;
                if fit.len() < min_cell || held.len() < min_cell {
                    continue;
                }
                let unit = spread(fit, (1.0, 0.0));
                let line = regress(fit);
                println!(
                    "  {d:2} {:2}  {:2}-{:2}  {:6} {:6}  {unit:7.1} {:7.1}  {:5.3} {:8.1}  {:7.1}",
                    rule(d),
                    BUCKETS[bucket].0,
                    BUCKETS[bucket].1,
                    fit.len(),
                    held.len(),
                    spread(held, (1.0, 0.0)),
                    line.0,
                    line.1,
                    spread(held, line)
                );
                rows.push((d, bucket, line));
            }
        }
        if name != "v1" || rows.is_empty() {
            continue;
        }
        let all_fit: Vec<(f64, f64)> = rows
            .iter()
            .flat_map(|&(d, b, _)| grid[d as usize][b].pairs[0].iter().copied())
            .collect();
        let pooled = regress(&all_fit);
        let linear = |line: (f64, f64)| {
            let fitted: Vec<(Vec<f64>, f64, f64)> = rows
                .iter()
                .map(|&(d, b, _)| {
                    let pairs = &grid[d as usize][b].pairs[0];
                    (
                        vec![1.0, midpoint(b), f64::from(d)],
                        spread(pairs, line),
                        pairs.len() as f64,
                    )
                })
                .collect();
            least_squares(&fitted)
        };
        let unit = linear((1.0, 0.0));
        let scaled = linear(pooled);
        let per_bucket: Vec<Vec<f64>> = (0..BUCKETS.len())
            .map(|bucket| {
                let fitted: Vec<(Vec<f64>, f64, f64)> = rows
                    .iter()
                    .filter(|row| row.1 == bucket)
                    .map(|&(d, b, _)| {
                        let pairs = &grid[d as usize][b].pairs[0];
                        (
                            vec![1.0, f64::from(d)],
                            spread(pairs, (1.0, 0.0)),
                            pairs.len() as f64,
                        )
                    })
                    .collect();
                if fitted.len() < 2 {
                    vec![0.0, 0.0]
                } else {
                    least_squares(&fitted)
                }
            })
            .collect();
        let forms = [
            Form {
                name: "linear a=1 b=0 (S0 + S1*e + S2*d)",
                sigma: Box::new({
                    let c = unit.clone();
                    move |d, b| c[0] + c[1] * midpoint(b) + c[2] * f64::from(d)
                }),
                line: (1.0, 0.0),
                constants: unit,
            },
            Form {
                name: "linear, pooled a b (S0 + S1*e + S2*d)",
                sigma: Box::new({
                    let c = scaled.clone();
                    move |d, b| c[0] + c[1] * midpoint(b) + c[2] * f64::from(d)
                }),
                line: pooled,
                constants: scaled,
            },
            Form {
                name: "per bucket a=1 b=0 (S0_k + S2_k*d)",
                sigma: Box::new({
                    let c = per_bucket.clone();
                    move |d, b| c[b][0] + c[b][1] * f64::from(d)
                }),
                line: (1.0, 0.0),
                constants: per_bucket.concat(),
            },
        ];
        for form in forms {
            // Inflate until model σ >= 95% of held-out σ in every supported cell.
            let k = rows
                .iter()
                .map(|&(d, b, _)| {
                    0.95 * spread(&grid[d as usize][b].pairs[1], form.line)
                        / (form.sigma)(d, b).max(1.0)
                })
                .fold(1.0f64, f64::max);
            let (mut high, mut low, mut n, mut sigma_sum, mut worst) = (0, 0, 0, 0.0, 0.0f64);
            for &(d, b, _) in &rows {
                let sigma = k * (form.sigma)(d, b).max(1.0);
                let held = &grid[d as usize][b].pairs[1];
                let (a, c) = form.line;
                let h = held
                    .iter()
                    .filter(|p| p.1 < a * p.0 + c - t * sigma)
                    .count();
                let l = held
                    .iter()
                    .filter(|p| p.1 > a * p.0 + c + t * sigma)
                    .count();
                worst = worst
                    .max(wilson_upper(h, held.len()))
                    .max(wilson_upper(l, held.len()));
                high += h;
                low += l;
                n += held.len();
                sigma_sum += sigma * held.len() as f64;
            }
            println!(
                "\nform {}: a {:.4} b {:.1} inflation {k:.3} mean_sigma {:.1}\n  held boundary errors at t {t:.2}: high {high}/{n} ({:.2}%) low {low}/{n} ({:.2}%) worst cell upper95 {:.2}%\n  constants (centidiscs, inflated): {:?}",
                form.name,
                form.line.0,
                form.line.1,
                sigma_sum / n as f64,
                100.0 * high as f64 / n as f64,
                100.0 * low as f64 / n as f64,
                100.0 * worst,
                form.constants
                    .iter()
                    .map(|c| (c * k * 100.0).round() / 100.0)
                    .collect::<Vec<_>>()
            );
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("collect") => collect(&args[1..]),
        Some("fit") => fit(&args[1..]),
        _ => Err("usage: probcut_fit collect --out FILE ... | fit --in FILE ...".into()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pairs `v_d = a·v_s + b + e` with e of spread `sigma`.
    fn synthetic(a: f64, b: f64, sigma: f64, n: usize) -> Vec<(f64, f64)> {
        let mut rng = Rng(1);
        (0..n)
            .map(|_| {
                let x = rng.below(4001) as f64 - 2000.0;
                // Sum of 12 uniforms in [0, 1) minus 6: mean 0, variance 1.
                let z: f64 = (0..12)
                    .map(|_| rng.below(1 << 20) as f64 / f64::from(1 << 20))
                    .sum::<f64>()
                    - 6.0;
                (x, a * x + b + sigma * z)
            })
            .collect()
    }

    #[test]
    fn regression_recovers_line_and_spread() {
        let pairs = synthetic(0.9, 50.0, 300.0, 20_000);
        let (a, b) = regress(&pairs);
        assert!((a - 0.9).abs() < 0.01, "a {a}");
        assert!((b - 50.0).abs() < 10.0, "b {b}");
        let sigma = spread(&pairs, (a, b));
        assert!((sigma - 300.0).abs() < 10.0, "sigma {sigma}");
    }

    #[test]
    fn least_squares_solves_exact_system() {
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
        let c = least_squares(&rows);
        for (got, want) in c.iter().zip([200.0, 3.0, -7.0]) {
            assert!((got - want).abs() < 1e-6, "{c:?}");
        }
    }

    #[test]
    fn wilson_bounds() {
        assert!((wilson_upper(0, 100) - 0.037).abs() < 0.001);
        assert!(wilson_upper(5, 100) > 0.05 && wilson_upper(5, 100) < 0.12);
        assert_eq!(wilson_upper(0, 0), 1.0);
    }

    #[test]
    fn shallow_rules_keep_parity() {
        for d in DEPTHS {
            for (name, rule) in RULES {
                let s = rule(d);
                assert!(s >= 1 && s < d, "{name} d {d} s {s}");
                if name == "v1" || d > 6 {
                    assert_eq!((d - s) % 2, 0, "{name} d {d} s {s}");
                }
            }
        }
    }
}
