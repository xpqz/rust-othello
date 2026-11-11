use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;
use std::time::Instant;

use othello::board::{Board, empty_count};
use othello::eval;
use othello::pattern;
use othello::score::EVAL_BOUND;
use othello_tools::provenance::git_commit;
use othello_tools::sym;
use sha2::{Digest, Sha256};

use crate::check;
use crate::wthor::{self, Game};

#[derive(Clone, Debug)]
pub struct Example {
    pub features: Vec<u32>,
    pub label: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub step: f32,
    pub floor: f32,
    pub patience: usize,
    pub max_passes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            step: 0.01,
            floor: 50.0,
            patience: 20,
            max_passes: 500,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Fit {
    pub values: Vec<f32>,
    pub curve: Vec<(f32, f32)>,
    pub best: usize,
}

pub fn fit(table: usize, train: &[Example], held: &[Example], options: &Options) -> Fit {
    let mut values = vec![0.0; table];
    let mut best_values = values.clone();
    let mut best_error = f32::INFINITY;
    let mut best = 0;
    let mut stale = 0;
    let mut curve = Vec::new();
    for pass in 0..options.max_passes {
        let mut sums = vec![0.0; table];
        let mut counts = vec![0u32; table];
        let mut train_abs = 0.0;
        for example in train {
            let residual = example.label - predict(&values, &example.features);
            train_abs += residual.abs();
            for &feature in &example.features {
                sums[feature as usize] += residual;
                counts[feature as usize] += 1;
            }
        }
        for index in 0..table {
            if counts[index] > 0 {
                let denominator = (counts[index] as f32).max(options.floor);
                values[index] += options.step * sums[index] / denominator;
            }
        }
        let train_error = if train.is_empty() {
            0.0
        } else {
            train_abs / train.len() as f32
        };
        let held_error = mae(&values, held);
        curve.push((train_error, held_error));
        if held_error < best_error {
            best_error = held_error;
            best_values = values.clone();
            best = pass;
            stale = 0;
        } else {
            stale += 1;
            if stale >= options.patience {
                break;
            }
        }
    }
    Fit {
        values: best_values,
        curve,
        best,
    }
}

pub fn predict(values: &[f32], features: &[u32]) -> f32 {
    features
        .iter()
        .map(|&feature| values[feature as usize])
        .sum()
}

fn mae(values: &[f32], examples: &[Example]) -> f32 {
    if examples.is_empty() {
        return 0.0;
    }
    examples
        .iter()
        .map(|example| (predict(values, &example.features) - example.label).abs())
        .sum::<f32>()
        / examples.len() as f32
}

pub fn quantise(values: &[f32]) -> Result<Vec<i16>, String> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let scaled = (value * 100.0).round();
            if !scaled.is_finite() || scaled.abs() > 32767.0 {
                return Err(format!("entry {index}: {value} is outside i16 centidiscs"));
            }
            Ok(scaled as i16)
        })
        .collect()
}

struct Layout {
    starts: Vec<u32>,
    perms: Vec<Vec<usize>>,
}

static LAYOUT: LazyLock<Layout> = LazyLock::new(build_layout);

pub fn canonical(feature: u32) -> u32 {
    let family = LAYOUT
        .starts
        .iter()
        .rposition(|&start| feature >= start)
        .unwrap_or(0);
    let offset = LAYOUT.starts[family];
    let local = feature - offset;
    let image = permute(local, &LAYOUT.perms[family]);
    offset + local.min(image)
}

fn permute(index: u32, perm: &[usize]) -> u32 {
    let mut image = 0;
    let mut rest = index;
    for &target in perm {
        let digit = rest % 3;
        rest /= 3;
        image += digit * 3u32.pow(target as u32);
    }
    image
}

fn build_layout() -> Layout {
    let mut starts = vec![0u32; pattern::FAMILIES.len()];
    for instance in pattern::instances() {
        starts[instance.family] = instance.offset;
    }
    let perms = pattern::FAMILIES
        .iter()
        .map(|base| family_perm(base))
        .collect();
    Layout { starts, perms }
}

fn family_perm(base: &[u8]) -> Vec<usize> {
    let mut sorted = base.to_vec();
    sorted.sort();
    for kind in 1..8 {
        let mapped: Vec<u8> = base
            .iter()
            .map(|&square| {
                sym::transform(
                    Board {
                        me: 1u64 << square,
                        opp: 0,
                    },
                    kind,
                )
                .me
                .trailing_zeros() as u8
            })
            .collect();
        let mut transformed = mapped.clone();
        transformed.sort();
        if transformed == sorted {
            let perm: Vec<usize> = mapped
                .iter()
                .map(|square| base.iter().position(|s| s == square).unwrap())
                .collect();
            if perm
                .iter()
                .enumerate()
                .any(|(index, &target)| index != target)
            {
                return perm;
            }
        }
    }
    (0..base.len()).collect()
}

pub fn asset(stages: &[Vec<f32>]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(stages.len() * pattern::TABLE_SLOTS * 2);
    for stage in stages {
        if stage.len() != pattern::TABLE_SLOTS {
            return Err("stage has the wrong table size".into());
        }
        let values = quantise(stage)?;
        for index in 0..pattern::TABLE_SLOTS {
            let value = values[canonical(index as u32) as usize];
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    Ok(bytes)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub values: Vec<f32>,
    pub counts: Vec<u32>,
    pub curve: Vec<(f32, f32)>,
}

pub fn inspect(stage: &Stage, family: usize, count: usize) -> String {
    let start = LAYOUT.starts[family];
    let end = start + 3u32.pow(pattern::FAMILIES[family].len() as u32);
    let total: u64 = (start..end)
        .map(|index| u64::from(stage.counts[index as usize]))
        .sum();
    let weighted: f64 = (start..end)
        .map(|index| {
            f64::from(stage.values[index as usize]) * f64::from(stage.counts[index as usize])
        })
        .sum();
    let mean = if total == 0 {
        0.0
    } else {
        weighted / total as f64
    };
    let mut entries: Vec<(f64, u32, u32)> = (start..end)
        .filter(|&index| stage.counts[index as usize] > 0)
        .map(|index| {
            (
                f64::from(stage.values[index as usize]) - mean,
                stage.counts[index as usize],
                index,
            )
        })
        .collect();
    entries.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out = format!(
        "family {family} squares {:?} relative to occurrence-weighted mean {mean:+.2}\n",
        pattern::FAMILIES[family]
    );
    let mut selected = Vec::new();
    selected.extend(entries.iter().take(count));
    selected.extend(entries.iter().rev().take(count));
    for &(value, occurrences, index) in &selected {
        out.push_str(&format!(
            "{} {value:+.2} count {occurrences}\n",
            shape(family, index)
        ));
    }
    out
}

fn shape(family: usize, index: u32) -> String {
    let mut rest = index;
    (0..pattern::FAMILIES[family].len())
        .map(|_| {
            let digit = rest % 3;
            rest /= 3;
            match digit {
                1 => 'X',
                2 => 'O',
                _ => '.',
            }
        })
        .collect()
}

pub struct Report {
    pub inputs: Vec<(String, String)>,
    pub constants: Options,
    pub stages: Vec<StageReport>,
    pub label_quality: f32,
    pub asset_sha: String,
}

pub struct StageReport {
    pub stage: usize,
    pub curve: Vec<(f32, f32)>,
    pub held: f32,
    pub rounded: f32,
    pub zero: f32,
    pub classic: f32,
    pub correlation: f32,
    pub nonzero: f32,
    pub largest: f32,
}

pub fn report(report: &Report) -> String {
    let mut out = String::new();
    for (name, value) in &report.inputs {
        out.push_str(&format!("{name} {value}\n"));
    }
    out.push_str(&format!(
        "constants step {} floor {} patience {} max {}\n",
        report.constants.step,
        report.constants.floor,
        report.constants.patience,
        report.constants.max_passes
    ));
    for stage in &report.stages {
        out.push_str(&format!("stage {}\n", stage.stage));
        for (pass, (train, held)) in stage.curve.iter().enumerate() {
            out.push_str(&format!("pass {pass} {train:.2} {held:.2}\n"));
        }
        out.push_str(&format!(
            "held {:.2} rounded {:.2} zero {:.2} classic {:.2} correlation {:.2} nonzero {:.2} largest {:.2}\n",
            stage.held, stage.rounded, stage.zero, stage.classic, stage.correlation, stage.nonzero, stage.largest
        ));
    }
    out.push_str(&format!(
        "label quality {:.2}\nasset {}\n",
        report.label_quality, report.asset_sha
    ));
    out
}

pub fn held_out(game: &Game) -> bool {
    let digest = Sha256::digest(&game.moves);
    u64::from_le_bytes(digest[..8].try_into().unwrap()) % 20 == 0
}

pub fn run(root: &Path, options: &Options) -> Result<(), String> {
    let games = load_games(&root.join("data/wthor"))?;
    let (labels, journal_sha) = load_labels(&root.join("data/labels.journal"))?;
    let manifest_sha = hash_file(&root.join("trainer/wthor.sha256"))?;
    let fingerprint = layout_fingerprint();
    let commit = git_commit();
    let (toolchain, target) = toolchain();
    let quality = label_quality(&games, &labels);
    let start = Instant::now();
    let mut stages = Vec::new();
    let mut reports = Vec::new();
    let mut total = 0;
    for stage in 0..pattern::STAGES {
        let mut train = Vec::new();
        let mut held = Vec::new();
        let mut counts = vec![0u32; pattern::TABLE_SLOTS];
        let mut classic = Corr::default();
        let mut classic_abs = 0.0;
        let mut zero_abs = 0.0;
        for (key, values) in &labels {
            let Some(game) = games.get(key) else {
                continue;
            };
            let replay = wthor::replay(game).map_err(|_| format!("{key}: replay rejected"))?;
            if replay.positions.len() != values.len() {
                return Err(format!("{key}: label count mismatch"));
            }
            let is_held = held_out(game);
            for (ply, &(board, _)) in replay.positions.iter().enumerate() {
                if pattern::stage(empty_count(board)) != stage {
                    continue;
                }
                let features: Vec<u32> = pattern::features(board)
                    .iter()
                    .map(|&feature| canonical(feature))
                    .collect();
                let label = values[ply];
                if is_held {
                    let value = eval::evaluate(board) as f32 / 100.0;
                    classic.add(value, label);
                    classic_abs += (value - label).abs();
                    zero_abs += label.abs();
                    held.push(Example { features, label });
                } else {
                    for &feature in &features {
                        counts[feature as usize] += 1;
                    }
                    train.push(Example { features, label });
                }
            }
        }
        if held.is_empty() {
            return Err(format!("stage {stage}: no held-out examples"));
        }
        let started = Instant::now();
        let fit = fit(pattern::TABLE_SLOTS, &train, &held, options);
        let error = mae(&fit.values, &held);
        let rounded: Vec<f32> = quantise(&fit.values)?
            .iter()
            .map(|value| *value as f32 / 100.0)
            .collect();
        let held_count = held.len() as f32;
        reports.push(StageReport {
            stage,
            curve: fit.curve.clone(),
            held: error,
            rounded: mae(&rounded, &held),
            zero: zero_abs / held_count,
            classic: classic_abs / held_count,
            correlation: classic.value(),
            nonzero: fit.values.iter().filter(|value| **value != 0.0).count() as f32
                / pattern::TABLE_SLOTS as f32,
            largest: fit
                .values
                .iter()
                .fold(0.0f32, |max, value| max.max(value.abs())),
        });
        stages.push(Stage {
            values: fit.values,
            counts,
            curve: fit.curve,
        });
        total += train.len() + held.len();
        println!(
            "stage {stage} train {} held {} passes {} seconds {:.2} held_mae {error:.4}",
            train.len(),
            held.len(),
            stages.last().unwrap().curve.len(),
            started.elapsed().as_secs_f64()
        );
    }
    for dir in ["assets", "data/fit", "docs/tuning"] {
        std::fs::create_dir_all(root.join(dir)).map_err(|error| error.to_string())?;
    }
    let values: Vec<Vec<f32>> = stages.iter().map(|stage| stage.values.clone()).collect();
    let asset = self::asset(&values)?;
    let asset_sha = check::digest(&asset);
    write(&root.join("assets/eval.bin"), &asset)?;
    write_counts(&root.join("data/fit/counts.bin"), &stages)?;
    write_layout(
        &root.join("assets/eval.layout"),
        &fingerprint,
        &asset_sha,
        &commit,
        &asset,
    )?;
    let report = Report {
        inputs: vec![
            ("trainer".into(), commit),
            ("toolchain".into(), toolchain),
            ("target".into(), target),
            (
                "profile".into(),
                if cfg!(debug_assertions) {
                    "debug".into()
                } else {
                    "release".into()
                },
            ),
            ("manifest".into(), manifest_sha),
            ("journal".into(), journal_sha),
            ("layout".into(), fingerprint),
            ("games".into(), labels.len().to_string()),
            ("rejected".into(), "0".into()),
            ("positions".into(), total.to_string()),
        ],
        constants: *options,
        stages: reports,
        label_quality: quality,
        asset_sha,
    };
    write(
        &root.join("docs/tuning/pattern_fit.log"),
        self::report(&report).as_bytes(),
    )?;
    println!(
        "total stages {} examples {total} seconds {:.2}",
        pattern::STAGES,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(check::digest(&bytes))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn toolchain() -> (String, String) {
    let version = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let verbose = Command::new("rustc")
        .arg("-vV")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let target = verbose
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap_or("unknown")
        .to_string();
    (version, target)
}

fn layout_fingerprint() -> String {
    let mut text = format!(
        "stages {} table {} digits 0,1,2\n",
        pattern::STAGES,
        pattern::TABLE_SLOTS
    );
    for instance in pattern::instances() {
        text.push_str(&format!(
            "{} {} {:?}\n",
            instance.family, instance.offset, instance.squares
        ));
    }
    check::digest(text.as_bytes())
}

fn label_quality(games: &BTreeMap<String, Game>, labels: &BTreeMap<String, Vec<f32>>) -> f32 {
    let mut correlation = Corr::default();
    for (key, values) in labels {
        let Some(game) = games.get(key) else {
            continue;
        };
        let Ok(replay) = wthor::replay(game) else {
            continue;
        };
        if replay.positions.len() != values.len() {
            continue;
        }
        if let Some((ply, &(_, black))) = replay
            .positions
            .iter()
            .enumerate()
            .find(|(_, (board, _))| empty_count(*board) <= 14)
        {
            let value = if black { values[ply] } else { -values[ply] };
            correlation.add(2.0 * game.theoretical as f32 - 64.0, value);
        }
    }
    correlation.value()
}

fn write_counts(path: &Path, stages: &[Stage]) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(stages.len() * pattern::TABLE_SLOTS * 4);
    for stage in stages {
        for &count in &stage.counts {
            bytes.extend_from_slice(&count.to_le_bytes());
        }
    }
    write(path, &bytes)
}

fn write_layout(
    path: &Path,
    fingerprint: &str,
    asset_sha: &str,
    commit: &str,
    asset: &[u8],
) -> Result<(), String> {
    let mut text = format!("layout {fingerprint}\nasset {asset_sha}\ncommit {commit}\n");
    for board in fixed_boards() {
        let stage = pattern::stage(empty_count(board));
        let value: i32 = pattern::features(board)
            .iter()
            .map(|&feature| {
                let offset = (stage * pattern::TABLE_SLOTS + feature as usize) * 2;
                i16::from_le_bytes([asset[offset], asset[offset + 1]]) as i32
            })
            .sum();
        let value = value.clamp(-EVAL_BOUND, EVAL_BOUND);
        text.push_str(&format!(
            "board {:016x} {:016x} {value}\n",
            board.me, board.opp
        ));
    }
    write(path, text.as_bytes())
}

fn fixed_boards() -> Vec<Board> {
    let mut boards = vec![Board::START];
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/corpus.txt"))
        .unwrap_or_default();
    boards.extend(
        text.lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let me = u64::from_str_radix(parts.next()?, 16).ok()?;
                let opp = u64::from_str_radix(parts.next()?, 16).ok()?;
                Some(Board { me, opp })
            })
            .filter(|board| (board.me | board.opp).count_ones() >= 40)
            .take(3),
    );
    boards
}

#[derive(Default)]
struct Corr {
    n: f64,
    x: f64,
    y: f64,
    xx: f64,
    yy: f64,
    xy: f64,
}

impl Corr {
    fn add(&mut self, x: f32, y: f32) {
        let (x, y) = (f64::from(x), f64::from(y));
        self.n += 1.0;
        self.x += x;
        self.y += y;
        self.xx += x * x;
        self.yy += y * y;
        self.xy += x * y;
    }

    fn value(&self) -> f32 {
        let denominator =
            ((self.n * self.xx - self.x * self.x) * (self.n * self.yy - self.y * self.y)).sqrt();
        if denominator == 0.0 {
            0.0
        } else {
            ((self.n * self.xy - self.x * self.y) / denominator) as f32
        }
    }
}

fn load_games(dir: &Path) -> Result<BTreeMap<String, Game>, String> {
    let mut games = BTreeMap::new();
    for path in check::wtb_files(dir)? {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid input file name")?
            .to_string();
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let (_, records) = wthor::read(&bytes)?;
        for (index, game) in records.into_iter().enumerate() {
            games.insert(format!("{name}:{index}"), game);
        }
    }
    Ok(games)
}

fn load_labels(path: &Path) -> Result<(BTreeMap<String, Vec<f32>>, String), String> {
    let (_, mut records) = othello_tools::journal::read(path)?;
    records.sort_by(|a, b| a.id.cmp(&b.id));
    let mut text = String::new();
    let mut labels = BTreeMap::new();
    for record in &records {
        text.push_str(&format!("{}\t{}\n", record.id, record.line));
        let values: Vec<f32> = record
            .line
            .split_whitespace()
            .map(|part| part.parse::<i32>().map(|value| value as f32))
            .collect::<Result<_, _>>()
            .map_err(|_| format!("{}: invalid label", record.id))?;
        labels.insert(record.id.clone(), values);
    }
    Ok((labels, check::digest(text.as_bytes())))
}

pub fn inspect_path(
    root: &Path,
    family: usize,
    stage: usize,
    count: usize,
) -> Result<String, String> {
    if stage >= pattern::STAGES {
        return Err("invalid stage".into());
    }
    let asset = std::fs::read(root.join("assets/eval.bin"))
        .map_err(|error| format!("assets/eval.bin: {error}"))?;
    let counts = std::fs::read(root.join("data/fit/counts.bin"))
        .map_err(|error| format!("data/fit/counts.bin: {error}"))?;
    let table = pattern::TABLE_SLOTS;
    if asset.len() != pattern::STAGES * table * 2 || counts.len() != pattern::STAGES * table * 4 {
        return Err("asset or counts file has the wrong size".into());
    }
    let start = stage * table;
    let values: Vec<f32> = (0..table)
        .map(|index| {
            let offset = (start + index) * 2;
            i16::from_le_bytes([asset[offset], asset[offset + 1]]) as f32 / 100.0
        })
        .collect();
    let counts: Vec<u32> = (0..table)
        .map(|index| {
            let offset = (start + index) * 4;
            u32::from_le_bytes([
                counts[offset],
                counts[offset + 1],
                counts[offset + 2],
                counts[offset + 3],
            ])
        })
        .collect();
    Ok(inspect(
        &Stage {
            values,
            counts,
            curve: Vec::new(),
        },
        family,
        count,
    ))
}
