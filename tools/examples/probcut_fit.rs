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

use std::collections::HashSet;
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
/// Cell acceptance: positions, σ floor, proxy error bound and its `t`.
const MIN_POSITIONS: usize = 60;
const FLOOR: f64 = 0.95;
const MAX_ERROR: f64 = 0.15;
const CHECK_T: f64 = 1.5;

/// A shallow-depth rule and the least depth at which it gives `1 <= s < d`
/// with `d - s` even.
type Rule = (&'static str, fn(u32) -> u32, u32);

const RULES: [Rule; 3] = [
    ("v1", |d| 2 * (d / 4) + (d & 1), 3),
    ("d-4", |d| d - 4, 5),
    ("d-6", |d| d - 6, 7),
];

/// Pairs `(v_s, v_d)` and the positions' empties, per split.
#[derive(Default, Clone)]
struct Cell {
    pairs: [Vec<(f64, f64)>; 3],
    empties: [Vec<u32>; 3],
}

/// `grid[d][bucket]` for depths from the rule's minimum, at most one position
/// per game in each cell (samples come sorted by unit id: the lowest is kept).
fn cells(samples: &[(usize, Sample)], (_, rule, least): Rule) -> Vec<Vec<Cell>> {
    let mut grid = vec![vec![Cell::default(); BUCKETS.len()]; *DEPTHS.end() as usize + 1];
    let mut used = HashSet::new();
    for (split, sample) in samples {
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
                cell.pairs[*split].push((f64::from(vs), f64::from(vd)));
                cell.empties[*split].push(sample.empties);
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

/// A fitted model: line (a, b) and σ = c0 + c1·e + c2·d, times `k`.
#[derive(Clone, Copy)]
struct Model {
    line: (f64, f64),
    c: [f64; 3],
    k: f64,
}

impl Model {
    fn sigma(&self, d: u32, bucket: usize) -> f64 {
        (self.k * (self.c[0] + self.c[1] * midpoint(bucket) + self.c[2] * f64::from(d))).max(1.0)
    }
}

/// Proxy boundary errors (high, low) at `t` for one cell's pairs.
fn errors(pairs: &[(f64, f64)], model: &Model, sigma: f64, t: f64) -> (usize, usize) {
    let (a, b) = model.line;
    let high = pairs
        .iter()
        .filter(|p| p.1 < a * p.0 + b - t * sigma)
        .count();
    let low = pairs
        .iter()
        .filter(|p| p.1 > a * p.0 + b + t * sigma)
        .count();
    (high, low)
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

/// Whether one cell passes on the given split with the model as inflated.
fn accepted(cell: &Cell, split: usize, model: &Model, d: u32, bucket: usize) -> bool {
    let pairs = &cell.pairs[split];
    if pairs.len() < MIN_POSITIONS {
        return false;
    }
    let sigma = model.sigma(d, bucket);
    let (high, low) = errors(pairs, model, sigma, CHECK_T);
    sigma >= FLOOR * spread(pairs, model.line)
        && wilson_upper(high, pairs.len()) <= MAX_ERROR
        && wilson_upper(low, pairs.len()) <= MAX_ERROR
}

/// The rectangle with the most cells within `allowed` whose every cell is
/// accepted on `split` after inflating the model just enough for the σ
/// floor over that rectangle; ties go to the smaller mean σ.
fn best_rect(
    grid: &[Vec<Cell>],
    split: usize,
    model: &Model,
    least: u32,
    allowed: &dyn Fn(u32, usize) -> bool,
) -> Option<(Rect, Model)> {
    let mut best: Option<(usize, f64, Rect, Model)> = None;
    let depths = least.max(*DEPTHS.start())..=*DEPTHS.end();
    for d0 in depths.clone() {
        for d1 in d0..=*depths.end() {
            for b0 in 0..BUCKETS.len() {
                for b1 in b0..BUCKETS.len() {
                    let rect = ((d0, d1), (b0, b1));
                    if rect_cells(rect).any(|(d, b)| {
                        !allowed(d, b) || grid[d as usize][b].pairs[split].len() < MIN_POSITIONS
                    }) {
                        continue;
                    }
                    let k = rect_cells(rect)
                        .map(|(d, b)| {
                            FLOOR * spread(&grid[d as usize][b].pairs[split], model.line)
                                / Model { k: 1.0, ..*model }.sigma(d, b)
                        })
                        .fold(1.0f64, f64::max);
                    let inflated = Model { k, ..*model };
                    if !rect_cells(rect)
                        .all(|(d, b)| accepted(&grid[d as usize][b], split, &inflated, d, b))
                    {
                        continue;
                    }
                    let count = rect_cells(rect).count();
                    let mean = rect_cells(rect)
                        .map(|(d, b)| inflated.sigma(d, b))
                        .sum::<f64>()
                        / count as f64;
                    if best
                        .as_ref()
                        .is_none_or(|&(c, m, r, _)| better((count, mean, rect), (c, m, r)))
                    {
                        best = Some((count, mean, rect, inflated));
                    }
                }
            }
        }
    }
    best.map(|(_, _, rect, model)| (rect, model))
}

/// The two forms fitted on the fit split over every cell with enough pairs.
fn forms(grid: &[Vec<Cell>]) -> Vec<(&'static str, Model)> {
    let cells: Vec<(u32, usize)> = DEPTHS
        .flat_map(|d| (0..BUCKETS.len()).map(move |b| (d, b)))
        .filter(|&(d, b)| grid[d as usize][b].pairs[0].len() >= MIN_POSITIONS)
        .collect();
    if cells.len() < 3 {
        return Vec::new();
    }
    let pooled: Vec<(f64, f64)> = cells
        .iter()
        .flat_map(|&(d, b)| grid[d as usize][b].pairs[0].iter().copied())
        .collect();
    let linear = |line: (f64, f64)| {
        let rows: Vec<(Vec<f64>, f64, f64)> = cells
            .iter()
            .map(|&(d, b)| {
                let pairs = &grid[d as usize][b].pairs[0];
                (
                    vec![1.0, midpoint(b), f64::from(d)],
                    spread(pairs, line),
                    pairs.len() as f64,
                )
            })
            .collect();
        let c = least_squares(&rows);
        Model {
            line,
            c: [c[0], c[1], c[2]],
            k: 1.0,
        }
    };
    let (a, b) = regress(&pooled);
    let mut out = vec![("a=1 b=0", linear((1.0, 0.0)))];
    if a > 0.0 {
        out.push(("pooled a b", linear((a, b))));
    }
    out
}

fn report_cells(name: &str, grid: &[Vec<Cell>], model: Option<&Model>, rule: fn(u32) -> u32) {
    println!(
        "\nrule {name}: per cell; spread of v_d - v_s on fit and selection (audit counted only){}",
        if model.is_some() { "; model sigma" } else { "" }
    );
    println!(
        "   d  s  bucket  n_fit n_sel n_aud  sd_fit  sd_sel{}",
        if model.is_some() { "   sigma" } else { "" }
    );
    for d in DEPTHS {
        for (bucket, cell) in grid[d as usize].iter().enumerate() {
            let [f, s, a] = &cell.pairs;
            if f.is_empty() {
                continue;
            }
            let unit = (1.0, 0.0);
            print!(
                "  {d:2} {:2}  {:2}-{:2}  {:5} {:5} {:5}  {:6.1}  {:6.1}",
                rule(d),
                BUCKETS[bucket].0,
                BUCKETS[bucket].1,
                f.len(),
                s.len(),
                a.len(),
                spread(f, unit),
                spread(s, unit)
            );
            match model {
                Some(model) => println!("  {:6.1}", model.sigma(d, bucket)),
                None => println!(),
            }
        }
    }
}

/// Empties bounds of a rectangle, intersected with the selection positions
/// in it, the solver threshold and the largest supported count.
fn empties_bounds(grid: &[Vec<Cell>], ((d0, d1), (b0, b1)): Rect) -> (u32, u32) {
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

fn fit(args: &[String]) -> Result<(), String> {
    let value = |flag: &str, default: &str| {
        let at = args.iter().position(|arg| arg == flag);
        at.and_then(|at| args.get(at + 1))
            .map_or(default.to_string(), String::clone)
    };
    let input = value("--in", "");
    if input.is_empty() {
        return Err("usage: probcut_fit fit --in FILE --dev FILE [--audit]".into());
    }
    let dev_path = value("--dev", "");
    let development: HashSet<usize> = std::fs::read_to_string(&dev_path)
        .map_err(|error| format!("--dev {dev_path}: {error}"))?
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            line.trim()
                .parse()
                .map_err(|_| format!("bad unit id {line}"))
        })
        .collect::<Result<_, _>>()?;
    let audit = args.iter().any(|arg| arg == "--audit");
    let raw = samples(Path::new(&input))?;
    let total = raw.len();
    let samples = distinct(raw, &development);
    let counts = [0, 1, 2].map(|s| samples.iter().filter(|(split, _)| *split == s).count());
    println!(
        "probcut_fit fit {input} records {total} development {} kept {} fit {} selection {} audit {} min_positions {MIN_POSITIONS} floor {FLOOR} max_error {MAX_ERROR} check_t {CHECK_T}",
        development.len(),
        samples.len(),
        counts[0],
        counts[1],
        counts[2]
    );
    let allowed = |_: u32, b: usize| BUCKETS[b].1 >= THRESHOLD + 2 && BUCKETS[b].0 <= MAX_EMPTIES;
    let mut chosen: Option<(usize, f64, &str, &str, Rect, Model)> = None;
    for rule in RULES {
        let grid = cells(&samples, rule);
        report_cells(rule.0, &grid, None, rule.1);
        for (form, model) in forms(&grid) {
            match best_rect(&grid, 1, &model, rule.2, &allowed) {
                Some((rect, inflated)) => {
                    let count = rect_cells(rect).count();
                    let mean = rect_cells(rect)
                        .map(|(d, b)| inflated.sigma(d, b))
                        .sum::<f64>()
                        / count as f64;
                    println!(
                        "rule {} form {form}: rectangle depths {}..={} buckets {}..={} cells {count} inflation {:.3} mean_sigma {mean:.1}",
                        rule.0,
                        rect.0.0,
                        rect.0.1,
                        BUCKETS[rect.1.0].0,
                        BUCKETS[rect.1.1].1,
                        inflated.k
                    );
                    if chosen
                        .as_ref()
                        .is_none_or(|c| better((count, mean, rect), (c.0, c.1, c.4)))
                    {
                        chosen = Some((count, mean, rule.0, form, rect, inflated));
                    }
                }
                None => println!("rule {} form {form}: no accepted rectangle", rule.0),
            }
        }
    }
    let Some((_, _, name, form, rect, model)) = chosen else {
        return Err("no rule and form has an accepted rectangle".into());
    };
    let rule = RULES.into_iter().find(|r| r.0 == name).unwrap();
    let grid = cells(&samples, rule);
    report_cells(name, &grid, Some(&model), rule.1);
    let ((d0, d1), (b0, b1)) = rect;
    let (e_min, e_max) = empties_bounds(&grid, rect);
    println!(
        "\nchosen (selection split): rule {name} form {form} a {:.4} b {:.1} inflation {:.3}\n  rectangle depths {d0}..={d1} empties {e_min}..={e_max}",
        model.line.0, model.line.1, model.k
    );
    let hundredths = model.c.map(|c| (c * model.k * 100.0).round() as i64);
    println!(
        "  ProbCut {{ t: T, s0: {}, s1: {}, s2: {}, a_milli: {}, b: {}, d_min: {d0}, d_max: {d1}, e_min: {e_min}, e_max: {e_max} }}",
        hundredths[0],
        hundredths[1],
        hundredths[2],
        (model.line.0 * 1000.0).round() as i64,
        model.line.1.round() as i64
    );
    if !audit {
        println!("audit split not opened (pass --audit once the choice is frozen)");
        return Ok(());
    }
    println!(
        "\naudit split, frozen model, per cell: positions, sigma floor, proxy errors at t {CHECK_T} with Wilson upper bounds"
    );
    let mut failed = Vec::new();
    for (d, b) in rect_cells(rect) {
        let pairs = &grid[d as usize][b].pairs[2];
        let sigma = model.sigma(d, b);
        let (high, low) = errors(pairs, &model, sigma, CHECK_T);
        let floor = sigma >= FLOOR * spread(pairs, model.line);
        println!(
            "  d {d:2} bucket {:2}-{:2} n {:4} sigma {sigma:6.1} audit_sd {:6.1} floor {} high {high} ({:.1}% up {:.1}%) low {low} ({:.1}% up {:.1}%)",
            BUCKETS[b].0,
            BUCKETS[b].1,
            pairs.len(),
            spread(pairs, model.line),
            if floor { "ok" } else { "FAIL" },
            100.0 * high as f64 / pairs.len().max(1) as f64,
            100.0 * wilson_upper(high, pairs.len()),
            100.0 * low as f64 / pairs.len().max(1) as f64,
            100.0 * wilson_upper(low, pairs.len())
        );
        if !floor {
            failed.push((d, b));
        }
    }
    if failed.is_empty() {
        println!("audit: every cell meets the floor; rectangle unchanged");
    } else {
        // Shrink by the rectangle order to a sub-rectangle without failed cells; no refit.
        let mut best: Option<(usize, f64, Rect)> = None;
        for e0 in d0..=d1 {
            for e1 in e0..=d1 {
                for c0 in b0..=b1 {
                    for c1 in c0..=b1 {
                        let sub = ((e0, e1), (c0, c1));
                        let count = rect_cells(sub).count();
                        let mean = rect_cells(sub).map(|(d, b)| model.sigma(d, b)).sum::<f64>()
                            / count as f64;
                        if rect_cells(sub).all(|cell| !failed.contains(&cell))
                            && best.is_none_or(|b| better((count, mean, sub), b))
                        {
                            best = Some((count, mean, sub));
                        }
                    }
                }
            }
        }
        match best {
            Some((_, _, sub)) => {
                let (low, high) = empties_bounds(&grid, sub);
                println!(
                    "audit: {} cells fail the floor; rectangle shrinks to depths {}..={} empties {low}..={high}",
                    failed.len(),
                    sub.0.0,
                    sub.0.1
                );
            }
            None => println!("audit: every cell fails; no rectangle"),
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
    fn synthetic(a: f64, b: f64, sigma: f64, n: usize, seed: u64) -> Vec<(f64, f64)> {
        let mut rng = Rng(seed);
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
        let pairs = synthetic(0.9, 50.0, 300.0, 20_000, 1);
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
    fn shallow_rules_valid_from_their_least_depth() {
        for (name, rule, least) in RULES {
            for d in least..=*DEPTHS.end() {
                let s = rule(d);
                assert!(s >= 1 && s < d && (d - s) % 2 == 0, "{name} d {d} s {s}");
            }
        }
    }

    #[test]
    fn canonical_is_symmetry_invariant() {
        let board = Board {
            me: 0x0000_0010_3800_0000,
            opp: 0x0000_0808_0400_0200,
        };
        let key = canonical(board);
        let images = [
            Board {
                me: flip_vertical(board.me),
                opp: flip_vertical(board.opp),
            },
            Board {
                me: mirror_horizontal(board.me),
                opp: mirror_horizontal(board.opp),
            },
            Board {
                me: transpose(board.me),
                opp: transpose(board.opp),
            },
            Board {
                me: transpose(flip_vertical(board.me)),
                opp: transpose(flip_vertical(board.opp)),
            },
        ];
        for image in images {
            assert_eq!(canonical(image), key);
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
    fn distinct_keeps_lowest_unit_and_one_split() {
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
    fn rectangle_excludes_a_noisy_cell() {
        // Every cell has sigma 300 except depth 6 in the last bucket (900).
        let mut grid = vec![vec![Cell::default(); BUCKETS.len()]; *DEPTHS.end() as usize + 1];
        for d in 3..=8u32 {
            for b in 1..BUCKETS.len() {
                let sigma = if d == 6 && b == 4 { 900.0 } else { 300.0 };
                for split in 0..3 {
                    grid[d as usize][b].pairs[split] = synthetic(
                        1.0,
                        0.0,
                        sigma,
                        200,
                        u64::from(d) * 10 + b as u64 + 100 * split as u64,
                    );
                }
            }
        }
        let model = Model {
            line: (1.0, 0.0),
            c: [300.0, 0.0, 0.0],
            k: 1.0,
        };
        let (rect, inflated) = best_rect(&grid, 1, &model, 3, &|_, _| true).expect("a rectangle");
        assert!(
            rect_cells(rect).all(|(d, b)| !(d == 6 && b == 4)),
            "{rect:?}"
        );
        assert_eq!(rect_cells(rect).count(), 18, "{rect:?}");
        assert!(inflated.k >= 1.0 && inflated.k < 1.1, "k {}", inflated.k);
    }

    #[test]
    fn development_units_never_audit() {
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
        let development = HashSet::from([1, 3]);
        let kept: Vec<(usize, usize)> = distinct(samples, &development)
            .iter()
            .map(|(f, s)| (*f, s.id))
            .collect();
        assert_eq!(kept, [(2, 2), (0, 3)]);
    }

    #[test]
    fn one_position_per_game_in_a_cell() {
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
    fn rectangle_order_and_empties_bounds() {
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
}
