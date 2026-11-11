use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use othello::board::{Board, empty_count};
use othello::pattern;
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
            step: 0.001,
            floor: 500.0,
            patience: 20,
            max_passes: 5000,
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
        for example in train {
            let residual = example.label - predict(&values, &example.features);
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
        let train_error = mae(&values, train);
        let held_error = mae(&values, if held.is_empty() { train } else { held });
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
    let mut starts = Vec::new();
    let mut perms = Vec::new();
    let mut offset = 0;
    for base in pattern::FAMILIES {
        starts.push(offset);
        offset += 3u32.pow(base.len() as u32);
        perms.push(family_perm(base));
    }
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
            return mapped
                .iter()
                .map(|square| base.iter().position(|s| s == square).unwrap())
                .collect();
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

pub fn save_stage(path: &Path, header: &str, stage: &Stage) -> Result<(), String> {
    let body = stage_body(stage);
    let hash = check::digest(body.as_bytes());
    let text = format!("{header}\n{hash}\n{body}");
    let temporary = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&temporary, text).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

pub fn load_stage(path: &Path, header: &str) -> Result<Option<Stage>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut lines = text.splitn(3, '\n');
    if lines.next() != Some(header) {
        return Ok(None);
    }
    let Some(hash) = lines.next() else {
        return Ok(None);
    };
    let Some(body) = lines.next() else {
        return Ok(None);
    };
    if check::digest(body.as_bytes()) != hash {
        return Ok(None);
    }
    Ok(parse_stage(body))
}

pub fn read_stage(path: &Path) -> Result<Option<Stage>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let body = text.splitn(3, '\n').nth(2).ok_or("short stage file")?;
    Ok(parse_stage(body))
}

fn stage_body(stage: &Stage) -> String {
    let values: Vec<String> = stage.values.iter().map(|value| value.to_string()).collect();
    let counts: Vec<String> = stage.counts.iter().map(|count| count.to_string()).collect();
    let curve: Vec<String> = stage
        .curve
        .iter()
        .flat_map(|(train, held)| [train.to_string(), held.to_string()])
        .collect();
    format!(
        "values {}\ncounts {}\ncurve {}\n",
        values.join(" "),
        counts.join(" "),
        curve.join(" ")
    )
}

fn parse_stage(body: &str) -> Option<Stage> {
    let mut values = None;
    let mut counts = None;
    let mut curve = Vec::new();
    for line in body.lines() {
        let mut parts = line.split_whitespace();
        match parts.next()? {
            "values" => {
                values = Some(
                    parts
                        .map(|part| part.parse().ok())
                        .collect::<Option<Vec<f32>>>()?,
                )
            }
            "counts" => {
                counts = Some(
                    parts
                        .map(|part| part.parse().ok())
                        .collect::<Option<Vec<u32>>>()?,
                )
            }
            "curve" => {
                let numbers: Vec<f32> = parts
                    .map(|part| part.parse().ok())
                    .collect::<Option<Vec<f32>>>()?;
                curve = numbers.chunks(2).map(|pair| (pair[0], pair[1])).collect();
            }
            _ => return None,
        }
    }
    Some(Stage {
        values: values?,
        counts: counts?,
        curve,
    })
}

pub fn inspect(stage: &Stage, family: usize, count: usize) -> String {
    let start = family_start(family);
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
    let mut out = format!("family {family} relative to occurrence-weighted mean {mean:+.2}\n",);
    let mut selected = Vec::new();
    selected.extend(entries.iter().take(count));
    selected.extend(entries.iter().rev().take(count));
    for &(value, occurrences, index) in &selected {
        out.push_str(&format!("config {index} {value:+.2} count {occurrences}\n"));
    }
    out
}

fn family_start(family: usize) -> u32 {
    LAYOUT.starts[family]
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
    let mut hash = Sha256::new();
    for &byte in &game.moves {
        hash.update([byte]);
    }
    let digest = hash.finalize();
    u64::from_le_bytes(digest[..8].try_into().unwrap()) % 20 == 0
}
