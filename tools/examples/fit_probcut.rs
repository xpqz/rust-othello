use othello::board::{
    Board, empty_count, final_score, is_terminal, legal_moves, pass, play, squares,
};
use othello::eval::evaluate;
use othello::probcut::{ProbCut, SigmaModel, shallow_depth};
use othello::rng::Rng;
use othello::score::{INF, terminal_score};
use othello::search::{Ctx, SearchConfig, SearchLimits, negamax, search_limited};
use othello::tt::{Table, zobrist};
use othello_tools::journal::Journal;
use othello_tools::positions;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const STAGES: [&str; 2] = ["selfplay.journal", "measure.journal"];
const T_VALUES: [f64; 5] = [1.0, 1.4, 1.5, 1.8, 2.2];

#[derive(Clone)]
struct Config {
    dir: PathBuf,
    games: usize,
    positions: usize,
    depth: u32,
    seed: u64,
    delay_ms: u64,
}

#[derive(Clone, Copy)]
struct Position {
    key: u64,
    game: u64,
    window: Option<(i32, i32)>,
}

#[derive(Clone, Copy)]
struct Sample {
    position: Position,
    board: Board,
}

#[derive(Clone)]
struct Measurement {
    position: Position,
    empties: u32,
    values: Vec<i32>,
}

#[derive(Clone, Copy, Default)]
struct DecisionCounts {
    high_fired: usize,
    false_high: usize,
    low_fired: usize,
    false_low: usize,
}

fn main() {
    let config = Config::parse();
    if let Err(error) = run_pipeline(&config) {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

impl Config {
    fn parse() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut config = Self {
            dir: PathBuf::from("probcut-run"),
            games: 200,
            positions: 4000,
            depth: 12,
            seed: 1,
            delay_ms: 0,
        };
        let mut index = 0;
        while index < args.len() {
            let value = &args[index + 1];
            match args[index].as_str() {
                "--dir" => config.dir = PathBuf::from(value),
                "--games" => config.games = value.parse().unwrap(),
                "--positions" => config.positions = value.parse().unwrap(),
                "--depth" => config.depth = value.parse().unwrap(),
                "--seed" => config.seed = value.parse().unwrap(),
                "--unit-delay-ms" => config.delay_ms = value.parse().unwrap(),
                flag => panic!("unknown option {flag}"),
            }
            index += 2;
        }
        config
    }

    fn header(&self) -> String {
        format!(
            "header seed {} games {} positions {} depth {} openings {} hash {:016x} engine {}",
            self.seed,
            self.games,
            self.positions,
            self.depth,
            openings_path().display(),
            file_hash(&openings_path()),
            git_commit()
        )
    }
}

fn openings_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bench/openings_tune.txt")
}

fn file_hash(path: &Path) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in fs::read(path).unwrap() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn git_commit() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn run_pipeline(config: &Config) -> Result<(), String> {
    fs::create_dir_all(&config.dir).map_err(|error| error.to_string())?;
    let games = self_play(config)?;
    let samples = sample_positions(config, &games)?;
    let measurements = measure_positions(config, &samples)?;
    let log = fit(config, &measurements);
    fs::write(config.dir.join("fit.log"), log).map_err(|error| error.to_string())
}

fn self_play(config: &Config) -> Result<Vec<Vec<u8>>, String> {
    let path = config.dir.join(STAGES[0]);
    let (mut journal, done) = Journal::open(&path, &config.header())?;
    let openings = positions::load(openings_path().to_str().unwrap())?;
    let mut games = Vec::new();
    for (game, opening) in openings.iter().take(config.games).enumerate() {
        let id = format!("game:{game}");
        if done.contains(&id) {
            continue;
        }
        let mut board = Board::START;
        let mut moves = Vec::new();
        for &square in &opening.squares {
            board = play(board, square);
            moves.push(square);
        }
        while !is_terminal(board) {
            let legal = legal_moves(board.me, board.opp);
            if legal == 0 {
                board = pass(board);
                moves.push(64);
                continue;
            }
            let limits = SearchLimits::new(config.depth.clamp(2, 6), u64::MAX, 0);
            let result = search_limited(board, limits, &SearchConfig::standard());
            board = play(board, result.best);
            moves.push(result.best);
        }
        let line = moves
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        journal.append(&id, &line)?;
    }
    drop(journal);
    let (_, records) = othello_tools::journal::read(&path)?;
    for record in records {
        games.push(
            record
                .line
                .split_whitespace()
                .map(|s| s.parse().unwrap())
                .collect(),
        );
    }
    Ok(games)
}

struct Reservoir {
    limit: usize,
    seen: BTreeSet<u64>,
    samples: Vec<Sample>,
    count: usize,
    rng: Rng,
}

impl Reservoir {
    fn offer(&mut self, sample: Sample) {
        if !self.seen.insert(sample.position.key) {
            return;
        }
        self.count += 1;
        if self.samples.len() < self.limit {
            self.samples.push(sample);
        } else {
            let pick = (self.rng.next_u64() as usize) % self.count;
            if pick < self.limit {
                self.samples[pick] = sample;
            }
        }
    }
}

fn sample_positions(config: &Config, games: &[Vec<u8>]) -> Result<Vec<Sample>, String> {
    let mut reservoir = Reservoir {
        limit: config.positions,
        seen: BTreeSet::new(),
        samples: Vec::new(),
        count: 0,
        rng: Rng::new(config.seed ^ 0x5A_11),
    };
    let search_depth = config.depth.clamp(4, 8);
    for (game, moves) in games.iter().enumerate() {
        let mut board = Board::START;
        let mut line = vec![board];
        for &square in moves {
            board = if square == 64 {
                pass(board)
            } else {
                play(board, square)
            };
            line.push(board);
        }
        for (index, board) in line.into_iter().enumerate() {
            reservoir.offer(Sample {
                position: Position {
                    key: zobrist(board),
                    game: game as u64,
                    window: None,
                },
                board,
            });
            if index % 3 == 0 {
                collect_nodes(board, search_depth, -INF, INF, game as u64, &mut reservoir);
            }
        }
    }
    if reservoir.samples.is_empty() {
        return Err("no positions sampled".into());
    }
    while reservoir.samples.len() < config.positions {
        let copy = reservoir.samples[reservoir.samples.len() % reservoir.samples.len()];
        reservoir.samples.push(copy);
    }
    Ok(reservoir.samples)
}

fn collect_nodes(
    board: Board,
    depth: u32,
    alpha: i32,
    beta: i32,
    game: u64,
    reservoir: &mut Reservoir,
) -> i32 {
    reservoir.offer(Sample {
        position: Position {
            key: zobrist(board),
            game,
            window: Some((alpha, beta)),
        },
        board,
    });
    let moves = legal_moves(board.me, board.opp);
    if moves == 0 {
        if legal_moves(board.opp, board.me) == 0 {
            return terminal_score(final_score(board));
        }
        return -collect_nodes(pass(board), depth, -beta, -alpha, game, reservoir);
    }
    if depth == 0 {
        return evaluate(board);
    }
    let mut best = -INF;
    let mut alpha = alpha;
    for square in squares(moves) {
        let value = -collect_nodes(
            play(board, square),
            depth - 1,
            -beta,
            -alpha,
            game,
            reservoir,
        );
        best = best.max(value);
        alpha = alpha.max(value);
        if alpha >= beta {
            break;
        }
    }
    best
}

fn measure_positions(config: &Config, samples: &[Sample]) -> Result<Vec<Measurement>, String> {
    let path = config.dir.join(STAGES[1]);
    let (mut journal, done) = Journal::open(&path, &config.header())?;
    let mut measurements = Vec::new();
    let mut written = 0;
    for sample in samples {
        let id = format!("{:016x}", sample.position.key);
        if !done.contains(&id) {
            let mut table = Table::new(1 << 20);
            let search = SearchConfig {
                endgame_threshold: 0,
                ..SearchConfig::standard()
            };
            let mut values = Vec::new();
            for depth in 1..=config.depth {
                let mut ctx = Ctx::new(&search);
                ctx.set_table(&mut table);
                let value =
                    negamax(sample.board, depth, -INF, INF, &mut ctx).ok_or("search aborted")?;
                values.push(value);
            }
            let window = sample.position.window.unwrap_or((-INF, INF));
            let payload = format!(
                "game {} empties {} window {} {} values {}",
                sample.position.game,
                empty_count(sample.board),
                window.0,
                window.1,
                values
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            journal.append(&id, &payload)?;
            written += 1;
            if config.delay_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(config.delay_ms));
            }
        }
        if written % 100 == 0 {
            eprintln!("measured {written}/{} positions", samples.len());
        }
    }
    drop(journal);
    let (_, records) = othello_tools::journal::read(&path)?;
    for record in records {
        measurements.push(parse_measurement(&record.id, &record.line)?);
    }
    measurements.sort_by_key(|measurement| measurement.position.key);
    Ok(measurements)
}

fn parse_measurement(id: &str, line: &str) -> Result<Measurement, String> {
    let key = u64::from_str_radix(id, 16).map_err(|_| "invalid key")?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 9
        || parts[0] != "game"
        || parts[2] != "empties"
        || parts[4] != "window"
        || parts[7] != "values"
    {
        return Err("malformed measurement".into());
    }
    Ok(Measurement {
        position: Position {
            key,
            game: parts[1].parse().map_err(|_| "invalid game")?,
            window: Some((
                parts[5].parse().map_err(|_| "invalid window")?,
                parts[6].parse().map_err(|_| "invalid window")?,
            )),
        },
        empties: parts[3].parse().map_err(|_| "invalid empties")?,
        values: parts[8..]
            .iter()
            .map(|value| value.parse().map_err(|_| "invalid value"))
            .collect::<Result<_, _>>()?,
    })
}

fn fit(config: &Config, measurements: &[Measurement]) -> String {
    let games: Vec<u64> = measurements
        .iter()
        .map(|m| m.position.game)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let (fit_games, held_games) = split_games(&games);
    let fit_set: BTreeSet<u64> = fit_games.iter().copied().collect();
    let held_set: BTreeSet<u64> = held_games.iter().copied().collect();
    let fit_rows: Vec<&Measurement> = measurements
        .iter()
        .filter(|m| fit_set.contains(&m.position.game))
        .collect();
    let held_rows: Vec<&Measurement> = measurements
        .iter()
        .filter(|m| held_set.contains(&m.position.game))
        .collect();
    let mut log = String::new();
    log.push_str(&format!(
        "seed {} games {} positions {} depth {}\n",
        config.seed, config.games, config.positions, config.depth
    ));
    let mut pairs = Vec::new();
    for row in &fit_rows {
        for depth in 2..=row.values.len().min(12) {
            let shallow = shallow_depth(depth as u32) as usize;
            if shallow == 0 || shallow > row.values.len() {
                continue;
            }
            pairs.push((row.values[shallow - 1] as f64, row.values[depth - 1] as f64));
            let bucket = row.empties.saturating_sub(18) / 6;
            let _ = bucket;
        }
    }
    let (a, b, sigma) = ols(&pairs);
    log.push_str(&format!(
        "global fit a={a:.6} b={b:.3} sigma={sigma:.3} r2={:.4}\n",
        r2(&pairs, a, b)
    ));
    let mut cells = BTreeMap::new();
    for row in &fit_rows {
        for depth in 2..=row.values.len().min(12) {
            let shallow = shallow_depth(depth as u32) as usize;
            if shallow == 0 || shallow > row.values.len() {
                continue;
            }
            let key = (depth, shallow, row.empties.saturating_sub(18) / 6);
            cells
                .entry(key)
                .or_insert_with(Vec::new)
                .push((row.values[shallow - 1] as f64, row.values[depth - 1] as f64));
        }
    }
    for ((depth, shallow, bucket), rows) in &cells {
        let (a, b, sigma) = ols(rows);
        log.push_str(&format!("cell d={depth} ds={shallow} bucket={bucket} n={} a={a:.4} b={b:.2} sigma={sigma:.2} r2={:.4}\n", rows.len(), r2(rows, a, b)));
    }
    let linear = SigmaModel::Linear {
        s0: sigma.round() as i32,
        gap: 0,
        empty: 0,
    };
    let edax = SigmaModel::EdaxStyle {
        a_milli: 1000,
        b_milli: 0,
        c_milli: 0,
        q2_milli: 0,
        q1_milli: 0,
        q0_milli: sigma.round() as i32 * 1000,
    };
    let table = SigmaModel::StageTable {
        buckets: [(sigma.round() as i32, 0); 6],
    };
    for (name, model) in [("linear", linear), ("edax", edax), ("stage", table)] {
        log.push_str(&format!("sigma-model {name} fitted {model:?}\n"));
    }
    let a_milli = (a * 1000.0).round().clamp(500.0, 1500.0) as i32;
    let b_int = b.round() as i32;
    let depth_min = 3;
    let depth_max = config.depth.min(12);
    let empty_min = measurements
        .iter()
        .map(|m| m.empties)
        .min()
        .unwrap_or(18)
        .max(18);
    let empty_max = measurements
        .iter()
        .map(|m| m.empties)
        .max()
        .unwrap_or(52)
        .min(52);
    log.push_str(&format!("supported-domain candidates depths {depth_min}..{depth_max} empties {empty_min}..{empty_max}\n"));
    let mut supported = Vec::new();
    for t in T_VALUES {
        let Ok(pc) = ProbCut::new(
            (t * 100.0) as u32,
            a_milli,
            b_int,
            linear,
            (depth_min, depth_max),
            (empty_min, empty_max),
        ) else {
            log.push_str(&format!("t={t:.1} invalid ProbCut parameters\n"));
            continue;
        };
        let mut high_n = 0;
        let mut high_bad = 0;
        let mut low_n = 0;
        let mut low_bad = 0;
        for row in &held_rows {
            let Some((alpha, beta)) = row.position.window else {
                continue;
            };
            for depth in 2..=row.values.len().min(12) {
                let shallow = shallow_depth(depth as u32) as usize;
                if shallow == 0 || shallow > row.values.len() {
                    continue;
                }
                let vs = row.values[shallow - 1];
                let vd = row.values[depth - 1];
                let counts =
                    decision_counts(&[(vs, vd, alpha, beta)], &pc, depth as u32, row.empties);
                high_n += counts.high_fired;
                high_bad += counts.false_high;
                low_n += counts.low_fired;
                low_bad += counts.false_low;
            }
        }
        let high_upper = cp_upper(high_bad as u32, high_n as u32);
        let low_upper = cp_upper(low_bad as u32, low_n as u32);
        log.push_str(&format!(
            "decisions t={t:.1} high {high_bad}/{high_n} upper {high_upper:.4} low {low_bad}/{low_n} upper {low_upper:.4} nominal {:.4}\n",
            1.0 - normal_cdf(t)
        ));
        if t == 1.5 && high_n >= 200 && low_n >= 200 && high_upper <= 0.12 && low_upper <= 0.12 {
            supported.push((depth_min, depth_max, empty_min, empty_max));
        }
    }
    if supported.is_empty() {
        log.push_str("supported-domain: none (epic stops pending design change)\n");
    } else {
        log.push_str("supported-domain: passes at t=1.5\n");
    }
    log.push_str(&format!(
        "PROBCUT_FIT = ProbCut::new({}, {}, {}, {:?}, ({}, {}), ({}, {})) // t=1.5\n",
        (1.5 * 100.0) as u32,
        a_milli,
        b_int,
        linear,
        depth_min,
        depth_max,
        empty_min,
        empty_max
    ));
    let residuals: Vec<f64> = pairs.iter().map(|(x, y)| (y - (a * x + b)).abs()).collect();
    let sigma_safe = sigma.max(1.0);
    log.push_str(&format!(
        "tails >2sigma {:.4} >3sigma {:.4}\n",
        residuals.iter().filter(|r| **r > 2.0 * sigma_safe).count() as f64
            / residuals.len().max(1) as f64,
        residuals.iter().filter(|r| **r > 3.0 * sigma_safe).count() as f64
            / residuals.len().max(1) as f64
    ));
    log
}

fn ols(pairs: &[(f64, f64)]) -> (f64, f64, f64) {
    if pairs.is_empty() {
        return (1.0, 0.0, 1.0);
    }
    let n = pairs.len() as f64;
    let mean_x = pairs.iter().map(|(x, _)| x).sum::<f64>() / n;
    let mean_y = pairs.iter().map(|(_, y)| y).sum::<f64>() / n;
    let sxx = pairs.iter().map(|(x, _)| (x - mean_x).powi(2)).sum::<f64>();
    let sxy = pairs
        .iter()
        .map(|(x, y)| (x - mean_x) * (y - mean_y))
        .sum::<f64>();
    let a = if sxx == 0.0 { 0.0 } else { sxy / sxx };
    let b = mean_y - a * mean_x;
    let variance = pairs
        .iter()
        .map(|(x, y)| (y - (a * x + b)).powi(2))
        .sum::<f64>()
        / n;
    (a, b, variance.sqrt().max(1.0))
}

fn r2(pairs: &[(f64, f64)], a: f64, b: f64) -> f64 {
    if pairs.is_empty() {
        return 0.0;
    }
    let mean = pairs.iter().map(|(_, y)| y).sum::<f64>() / pairs.len() as f64;
    let total: f64 = pairs.iter().map(|(_, y)| (y - mean).powi(2)).sum();
    let residual: f64 = pairs.iter().map(|(x, y)| (y - (a * x + b)).powi(2)).sum();
    if total == 0.0 {
        0.0
    } else {
        1.0 - residual / total
    }
}

fn split_games(games: &[u64]) -> (Vec<u64>, Vec<u64>) {
    let mut sorted = games.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let cut = sorted.len() * 7 / 10;
    (sorted[..cut].to_vec(), sorted[cut..].to_vec())
}

#[allow(dead_code)]
fn dedup_positions(rows: Vec<Position>) -> Vec<Position> {
    let mut seen = BTreeSet::new();
    rows.into_iter()
        .filter(|row| seen.insert(row.key))
        .collect()
}

fn decision_counts(
    rows: &[(i32, i32, i32, i32)],
    pc: &ProbCut,
    depth: u32,
    empties: u32,
) -> DecisionCounts {
    let mut counts = DecisionCounts::default();
    for &(vs, vd, alpha, beta) in rows {
        if pc
            .high(beta, depth, empties)
            .is_some_and(|high| vs >= high as i32)
        {
            counts.high_fired += 1;
            counts.false_high += usize::from(vd < beta);
        }
        if pc
            .low(alpha, depth, empties)
            .is_some_and(|low| vs <= low as i32)
        {
            counts.low_fired += 1;
            counts.false_low += usize::from(vd > alpha);
        }
    }
    counts
}

fn cp_upper(errors: u32, n: u32) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if errors == 0 {
        return 1.0 - 0.05f64.powf(1.0 / n as f64);
    }
    let mut low = errors as f64 / n as f64;
    let mut high = 1.0;
    for _ in 0..80 {
        let mid = (low + high) / 2.0;
        if binomial_cdf(errors, n, mid) > 0.05 {
            low = mid;
        } else {
            high = mid;
        }
    }
    high
}

fn binomial_cdf(k: u32, n: u32, p: f64) -> f64 {
    (0..=k)
        .map(|i| binomial(n, i) * p.powi(i as i32) * (1.0 - p).powi((n - i) as i32))
        .sum()
}

fn binomial(n: u32, k: u32) -> f64 {
    (1..=k).fold(1.0, |value, i| value * (n - k + i) as f64 / i as f64)
}

fn normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
            + 0.254829592)
            * t
            * (-x * x).exp();
    if x >= 0.0 { y } else { -y }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn ols_recovers_synthetic_fit() {
        let (a, b, sigma) = ols(&synthetic_data(0x5EED, 0.9, 20.0, 100.0, 4000));
        assert!((a - 0.9).abs() < 0.01, "{a}");
        assert!((b - 20.0).abs() < 5.0, "{b}");
        assert!((sigma - 100.0).abs() < 5.0, "{sigma}");
    }

    #[test]
    fn decision_counts_hand_table() {
        let pc = ProbCut::new(
            150,
            1000,
            0,
            SigmaModel::Linear {
                s0: 100,
                gap: 0,
                empty: 0,
            },
            (3, 12),
            (18, 52),
        )
        .unwrap();
        let rows = [
            (300, 90, 0, 100),
            (-300, -90, -100, 0),
            (300, 110, 0, 100),
            (-300, -110, -100, 0),
            (250, 100, 0, 100),
            (-250, -100, -100, 0),
        ];
        let counts = decision_counts(&rows, &pc, 8, 30);
        assert_eq!(counts.high_fired, 3);
        assert_eq!(counts.false_high, 1);
        assert_eq!(counts.low_fired, 3);
        assert_eq!(counts.false_low, 1);
    }

    #[test]
    fn clopper_pearson_upper_bounds() {
        assert!((cp_upper(0, 200) - 0.0149).abs() < 0.001);
        assert!((cp_upper(0, 75) - 0.0389).abs() < 0.002);
        assert!((cp_upper(10, 200) - 0.08334).abs() < 0.001);
        assert!((cp_upper(20, 200) - 0.14199).abs() < 0.001);
        assert!((cp_upper(5, 75) - 0.13507).abs() < 0.001);
    }

    #[test]
    fn split_is_grouped_and_stable() {
        let games: Vec<u64> = (0..100).collect();
        let (fit, held) = split_games(&games);
        let fit_set: BTreeSet<_> = fit.iter().copied().collect();
        let held_set: BTreeSet<_> = held.iter().copied().collect();
        assert!(fit_set.is_disjoint(&held_set));
        assert_eq!(fit_set.len() + held_set.len(), 100);
        assert!((fit.len() as f64 / 100.0 - 0.7).abs() < 0.05);
        assert_eq!(split_games(&games), (fit, held));
    }

    #[test]
    fn dedup_keeps_one_position_per_key() {
        let rows = vec![
            Position {
                key: 1,
                game: 1,
                window: None,
            },
            Position {
                key: 1,
                game: 2,
                window: None,
            },
            Position {
                key: 2,
                game: 1,
                window: Some((-100, 100)),
            },
        ];
        let unique = dedup_positions(rows);
        assert_eq!(unique.len(), 2);
        assert_eq!(unique.iter().find(|row| row.key == 1).unwrap().game, 1);
    }

    #[test]
    fn pipeline_interrupt_resume_and_header() {
        let unique = |name: &str| {
            std::env::temp_dir().join(format!(
                "fit_probcut_{}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                name
            ))
        };
        let example = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples/fit_probcut");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/fit_probcut.rs");
        assert!(
            example.metadata().unwrap().modified().unwrap()
                >= source.metadata().unwrap().modified().unwrap(),
            "fit_probcut example binary is missing or stale; run cargo build --release --example fit_probcut"
        );
        let run = |dir: &Path, seed: u64, depth: &str| {
            Command::new(&example)
                .args([
                    "--dir",
                    dir.to_str().unwrap(),
                    "--games",
                    "4",
                    "--positions",
                    "40",
                    "--depth",
                    depth,
                    "--unit-delay-ms",
                    "20",
                    "--seed",
                    &seed.to_string(),
                ])
                .output()
                .unwrap()
        };
        let reference = unique("reference");
        let resumed = unique("resumed");
        fs::create_dir_all(&reference).unwrap();
        fs::create_dir_all(&resumed).unwrap();
        assert!(run(&reference, 7, "4").status.success());
        let mut child = Command::new(&example)
            .args([
                "--dir",
                resumed.to_str().unwrap(),
                "--games",
                "4",
                "--positions",
                "40",
                "--depth",
                "4",
                "--unit-delay-ms",
                "20",
                "--seed",
                "7",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let measure = resumed.join("measure.journal");
        let start = Instant::now();
        loop {
            let lines = fs::read_to_string(&measure).map_or(0, |text| text.lines().count());
            if lines > 2 {
                child.kill().unwrap();
                break;
            }
            if child.try_wait().unwrap().is_some() || start.elapsed() > Duration::from_secs(30) {
                panic!("pipeline finished before interruption");
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(child.wait().unwrap().signal(), Some(9));
        assert!(run(&resumed, 7, "4").status.success());
        let reference_fit = fs::read(reference.join("fit.log")).unwrap();
        assert_eq!(fs::read(resumed.join("fit.log")).unwrap(), reference_fit);
        let text = fs::read_to_string(&measure).unwrap();
        let mut cut = text[..text.len().saturating_sub(4)].to_string();
        cut.push_str("torn");
        fs::write(&measure, cut).unwrap();
        assert!(run(&resumed, 7, "4").status.success());
        assert_eq!(fs::read(resumed.join("fit.log")).unwrap(), reference_fit);
        let mismatch = run(&resumed, 8, "4");
        assert!(!mismatch.status.success());
        assert!(String::from_utf8_lossy(&mismatch.stderr).contains("header"));
        let depth = run(&resumed, 7, "5");
        assert!(!depth.status.success());
        assert!(String::from_utf8_lossy(&depth.stderr).contains("header"));
        fs::remove_dir_all(reference).unwrap();
        fs::remove_dir_all(resumed).unwrap();
    }

    fn synthetic_data(seed: u64, a: f64, b: f64, sigma: f64, count: usize) -> Vec<(f64, f64)> {
        let mut rng = Rng::new(seed);
        (0..count)
            .map(|_| {
                let x = (rng.next_u64() % 12_801) as f64 - 6_400.0;
                let u1 = (rng.next_u64() as f64 + 1.0) / (u64::MAX as f64 + 2.0);
                let u2 = (rng.next_u64() as f64 + 1.0) / (u64::MAX as f64 + 2.0);
                let noise = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos() * sigma;
                (x, a * x + b + noise)
            })
            .collect()
    }
}
