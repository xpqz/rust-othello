use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;
use std::time::Instant;

use othello::board::{Board, empty_count};
use othello::pattern;
use othello::score::EVAL_BOUND;
use othello_tools::baselines::eval_classic;
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
            step: 0.02,
            floor: 200.0,
            patience: 100,
            max_passes: 1000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Fit {
    pub values: Vec<f32>,
    pub curve: Vec<(f32, f32)>,
    pub best: usize,
}

const MAX_WINDOW: usize = 100;

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
        "family {family} squares {:?} (X mover, O opponent, . empty) relative to occurrence-weighted mean {mean:+.2}\n",
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
    let rejected = rejected_count(&games, &labels);
    let positions: usize = labels.iter().map(|(_, values)| values.len()).sum();
    let mut report = format!(
        "trainer {commit}\ntoolchain {toolchain}\ntarget {target}\nprofile {}\nmanifest {manifest_sha}\njournal {journal_sha}\nlayout {fingerprint}\ngames {}\nrejected {rejected}\npositions {positions}\nconstants step {} floor {} patience {} max {}\n",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        labels.len(),
        options.step,
        options.floor,
        options.patience,
        options.max_passes
    );
    let mut quality = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stages = Vec::new();
    let start = Instant::now();
    let mut total = 0;
    for stage in 0..pattern::STAGES {
        let data = stage_data(stage, &games, &labels, &mut quality, &mut seen)?;
        if data.held.is_empty() {
            return Err(format!("stage {stage}: no held-out examples"));
        }
        let started = Instant::now();
        let fit = fit(pattern::TABLE_SLOTS, &data.train, &data.held, options);
        let held_count = data.held.len() as f32;
        let trained: Vec<(f32, f32)> = data
            .held
            .iter()
            .map(|example| (predict(&fit.values, &example.features), example.label))
            .collect();
        let rounded: Vec<f32> = quantise(&fit.values)?
            .iter()
            .map(|value| *value as f32 / 100.0)
            .collect();
        let error = mae(&fit.values, &data.held);
        let max_improvement = if fit.curve.len() == options.max_passes {
            let before = fit.curve[fit.curve.len().saturating_sub(MAX_WINDOW)].1;
            let after = fit.curve.last().unwrap().1;
            let improvement = before - after;
            if improvement > 0.01 {
                return Err(format!(
                    "stage {stage}: held improvement {improvement:.4} at max"
                ));
            }
            Some(improvement)
        } else {
            None
        };
        report.push_str(&format!("stage {stage}\n"));
        for (pass, (train, held)) in fit.curve.iter().enumerate() {
            report.push_str(&format!("pass {pass} {train:.2} {held:.2}\n"));
        }
        let max_text = max_improvement
            .map(|value| format!(" max_improvement {value:.4} window {MAX_WINDOW}"))
            .unwrap_or_default();
        report.push_str(&format!(
            "held {error:.2} rounded {:.2} zero {:.2} classic {:.2} trained {:.2} classic_corr {:.2} nonzero {:.2} largest {:.2}{max_text}\n",
            mae(&rounded, &data.held),
            data.zero_abs / held_count,
            data.classic_abs / held_count,
            correlation(&trained),
            correlation(&data.classic),
            fit.values.iter().filter(|value| **value != 0.0).count() as f32
                / pattern::TABLE_SLOTS as f32,
            fit.values
                .iter()
                .fold(0.0f32, |max, value| max.max(value.abs()))
        ));
        total += data.train.len() + data.held.len();
        stages.push(Stage {
            values: fit.values,
            counts: data.counts,
            curve: fit.curve,
        });
        println!(
            "stage {stage} train {} held {} passes {} seconds {:.2} held_mae {error:.4}",
            data.train.len(),
            data.held.len(),
            stages.last().unwrap().curve.len(),
            started.elapsed().as_secs_f64()
        );
    }
    for dir in ["data/fit", "docs/tuning"] {
        std::fs::create_dir_all(root.join(dir)).map_err(|error| error.to_string())?;
    }
    let values: Vec<Vec<f32>> = stages.iter().map(|stage| stage.values.clone()).collect();
    let asset = self::asset(&values)?;
    let asset_sha = check::digest(&asset);
    write(&root.join("data/fit/eval.bin"), &asset)?;
    write_counts(&root.join("data/fit/counts.bin"), &stages)?;
    write_layout(
        &root.join("data/fit/eval.layout"),
        &fingerprint,
        &asset_sha,
        &commit,
        &asset,
    )?;
    report.push_str(&format!(
        "label quality {:.2}\nasset {asset_sha}\n",
        correlation(&quality)
    ));
    write(&root.join("docs/tuning/pattern_fit.log"), report.as_bytes())?;
    println!(
        "total stages {} examples {total} seconds {:.2}",
        pattern::STAGES,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

struct StageData {
    train: Vec<Example>,
    held: Vec<Example>,
    counts: Vec<u32>,
    classic: Vec<(f32, f32)>,
    classic_abs: f32,
    zero_abs: f32,
}

fn stage_data(
    stage: usize,
    games: &BTreeMap<String, Game>,
    labels: &Labels,
    quality: &mut Vec<(f32, f32)>,
    seen: &mut BTreeSet<String>,
) -> Result<StageData, String> {
    let mut data = StageData {
        train: Vec::new(),
        held: Vec::new(),
        counts: vec![0u32; pattern::TABLE_SLOTS],
        classic: Vec::new(),
        classic_abs: 0.0,
        zero_abs: 0.0,
    };
    for (key, values) in labels {
        let Some(game) = games.get(key) else {
            continue;
        };
        let replay = wthor::replay(game).map_err(|_| format!("{key}: replay rejected"))?;
        if replay.positions.len() != values.len() {
            return Err(format!("{key}: label count mismatch"));
        }
        let is_held = held_out(game);
        for (ply, &(board, black)) in replay.positions.iter().enumerate() {
            if empty_count(board) <= 14 && seen.insert(key.clone()) {
                let value = if black { values[ply] } else { -values[ply] };
                quality.push((2.0 * game.theoretical as f32 - 64.0, value));
            }
            if pattern::stage(empty_count(board)) != stage {
                continue;
            }
            let features: Vec<u32> = pattern::features(board)
                .iter()
                .map(|&feature| canonical(feature))
                .collect();
            let label = values[ply];
            if is_held {
                let value = eval_classic(board) as f32 / 100.0;
                data.classic.push((value, label));
                data.classic_abs += (value - label).abs();
                data.zero_abs += label.abs();
                data.held.push(Example { features, label });
            } else {
                for &feature in &features {
                    data.counts[feature as usize] += 1;
                }
                data.train.push(Example { features, label });
            }
        }
    }
    Ok(data)
}

fn correlation(pairs: &[(f32, f32)]) -> f32 {
    let n = pairs.len() as f64;
    if n == 0.0 {
        return 0.0;
    }
    let (mut x, mut y, mut xx, mut yy, mut xy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for &(a, b) in pairs {
        let (a, b) = (f64::from(a), f64::from(b));
        x += a;
        y += b;
        xx += a * a;
        yy += b * b;
        xy += a * b;
    }
    let denominator = ((n * xx - x * x) * (n * yy - y * y)).sqrt();
    if denominator == 0.0 {
        0.0
    } else {
        ((n * xy - x * y) / denominator) as f32
    }
}

fn hash_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(check::digest(&bytes))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn toolchain() -> (String, String) {
    let text = Command::new("rustc")
        .arg("-vV")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let version = text.lines().next().unwrap_or("rustc unknown").to_string();
    let target = text
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap_or("unknown")
        .to_string();
    (version, target)
}

pub fn layout_fingerprint() -> String {
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

fn write_counts(path: &Path, stages: &[Stage]) -> Result<(), String> {
    let bytes: Vec<u8> = stages
        .iter()
        .flat_map(|stage| stage.counts.iter().flat_map(|count| count.to_le_bytes()))
        .collect();
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
    for board in FIXED_BOARDS {
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

const FIXED_BOARDS: [Board; 4] = [
    Board::START,
    Board {
        me: 0x785c68e1f14f0d11,
        opp: 0x0223171e0e3070a0,
    },
    Board {
        me: 0x03e723c0c88c1000,
        opp: 0xd0181c3e37726e7e,
    },
    Board {
        me: 0x7c03370918b4f8e4,
        opp: 0x80fcc8f6e64b0612,
    },
];

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

type Labels = Vec<(String, Vec<f32>)>;

fn load_labels(path: &Path) -> Result<(Labels, String), String> {
    let (_, records) = othello_tools::journal::read(path)?;
    let mut parsed = Vec::new();
    for record in records {
        let Some((file, index)) = key_parts(&record.id) else {
            return Err(format!("{}: invalid key", record.id));
        };
        let values: Vec<f32> = record
            .line
            .split_whitespace()
            .map(|part| part.parse::<i32>().map(|value| value as f32))
            .collect::<Result<_, _>>()
            .map_err(|_| format!("{}: invalid label", record.id))?;
        parsed.push((file.to_string(), index, record.id, record.line, values));
    }
    parsed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut text = String::new();
    let mut labels = Vec::new();
    for (_, _, id, line, values) in parsed {
        text.push_str(&format!("{id}\t{line}\n"));
        labels.push((id, values));
    }
    Ok((labels, check::digest(text.as_bytes())))
}

fn key_parts(key: &str) -> Option<(&str, usize)> {
    let (file, index) = key.rsplit_once(':')?;
    Some((file, index.parse().ok()?))
}

fn rejected_count(games: &BTreeMap<String, Game>, labels: &Labels) -> usize {
    let Some((last, _)) = labels.last() else {
        return 0;
    };
    let Some(max) = key_parts(last) else {
        return 0;
    };
    games
        .keys()
        .filter(|key| key_parts(key) <= Some(max))
        .count()
        .saturating_sub(labels.len())
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
    let asset = std::fs::read(root.join("data/fit/eval.bin"))
        .map_err(|error| format!("data/fit/eval.bin: {error}"))?;
    let counts = std::fs::read(root.join("data/fit/counts.bin"))
        .map_err(|error| format!("data/fit/counts.bin: {error}"))?;
    let table = pattern::TABLE_SLOTS;
    if asset.len() != pattern::STAGES * table * 2 || counts.len() != pattern::STAGES * table * 4 {
        return Err("asset or counts file has the wrong size".into());
    }
    let start = stage * table;
    let values = asset[start * 2..(start + table) * 2]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| i16::from_le_bytes(*bytes) as f32 / 100.0)
        .collect();
    let counts = counts[start * 4..(start + table) * 4]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| u32::from_le_bytes(*bytes))
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
