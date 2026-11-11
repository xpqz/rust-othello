use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use othello::board::{empty_count, final_score};
use sha2::{Digest, Sha256};

use crate::wthor::{self, Game, Reject};

#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub p3_values: BTreeMap<u8, usize>,
    pub metrics: BTreeMap<&'static str, (usize, usize)>,
}

impl Summary {
    fn add(&mut self, label: &'static str, agree: usize, total: usize) {
        let value = self.metrics.entry(label).or_default();
        value.0 += agree;
        value.1 += total;
    }
}

pub fn summarise(p3: u8, records: &[Game], summary: &mut Summary) {
    summary.add("games", records.len(), 0);
    *summary.p3_values.entry(p3).or_default() += 1;
    for game in records {
        let replay = match wthor::replay(game) {
            Ok(replay) => replay,
            Err(Reject::Illegal { .. }) => {
                summary.add("rejected illegal", 1, 0);
                continue;
            }
            Err(Reject::Incomplete) => {
                summary.add("rejected incomplete", 1, 0);
                continue;
            }
        };
        summary.add("replayed", 1, 0);
        let value = final_score(replay.final_board);
        let agree =
            (if replay.final_black { value } else { -value }) == 2 * i32::from(game.real) - 64;
        summary.add("real", usize::from(agree), 1);
        summary.add("theoretical", usize::from(game.theoretical == game.real), 1);
        if empty_count(replay.final_board) == 0 {
            summary.add("full board", usize::from(agree), 1);
        } else {
            summary.add("empty board", usize::from(agree), 1);
            if game.real > 32 {
                summary.add("empty black win", usize::from(agree), 1);
            } else if game.real < 32 {
                summary.add("empty black loss", usize::from(agree), 1);
            }
        }
    }
}

pub fn run(dir: &Path, manifest_path: &Path) -> Result<Summary, String> {
    let manifest = std::fs::read_to_string(manifest_path)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    let files = wtb_files(dir)?;
    verify_files(&files, &manifest)?;
    let mut summary = Summary::default();
    for path in files {
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let (p3, records) = wthor::read(&bytes)?;
        summarise(p3, &records, &mut summary);
    }
    Ok(summary)
}

pub fn verify_file(path: &Path, expected: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    (digest(&bytes) == expected.to_ascii_lowercase())
        .then_some(())
        .ok_or_else(|| format!("{}: SHA-256 mismatch", path.display()))
}

pub fn verify_manifest(dir: &Path, manifest: &str) -> Result<(), String> {
    verify_files(&wtb_files(dir)?, manifest)
}

pub(crate) fn wtb_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("wtb"))
        .collect();
    files.sort();
    Ok(files)
}

fn verify_files(files: &[PathBuf], manifest: &str) -> Result<(), String> {
    let names: BTreeSet<&Path> = files.iter().map(PathBuf::as_path).collect();
    let mut listed = BTreeSet::new();
    for line in manifest.lines() {
        let mut fields = line.split_whitespace();
        let (Some(hash), Some(name)) = (fields.next(), fields.next()) else {
            continue;
        };
        let name = name.strip_prefix('*').unwrap_or(name);
        let path = files
            .iter()
            .find(|path| path.ends_with(name))
            .ok_or_else(|| format!("{name}: missing"))?;
        verify_file(path, hash)?;
        listed.insert(path.as_path());
    }
    (listed.len() == names.len())
        .then_some(())
        .ok_or_else(|| "manifest does not list every .wtb file".into())
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes).as_slice())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
