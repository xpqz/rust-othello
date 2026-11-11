use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use othello::board::{empty_count, final_score};
use sha2::{Digest, Sha256};

use crate::wthor::{self, Database, Reject};

#[derive(Clone, Copy, Debug, Default)]
pub struct Group {
    pub agree: usize,
    pub total: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub games_read: usize,
    pub replayed: usize,
    pub rejected_illegal: usize,
    pub rejected_incomplete: usize,
    pub real_agree: usize,
    pub theoretical_equal: usize,
    pub p3_values: BTreeMap<u8, usize>,
    pub full_board: Group,
    pub empty_board: Group,
    pub black_win: Group,
    pub black_loss: Group,
}

pub fn summarise(db: &Database) -> Summary {
    let mut summary = Summary {
        games_read: db.games.len(),
        ..Summary::default()
    };
    *summary.p3_values.entry(db.header.p3).or_default() += 1;
    for game in &db.games {
        let replay = match wthor::replay(game) {
            Ok(replay) => replay,
            Err(Reject::Illegal { .. }) => {
                summary.rejected_illegal += 1;
                continue;
            }
            Err(Reject::Incomplete) => {
                summary.rejected_incomplete += 1;
                continue;
            }
        };
        summary.replayed += 1;
        let value = final_score(replay.final_board);
        let score = if replay.final_black { value } else { -value };
        let agree = score == 2 * i32::from(game.real) - 64;
        if agree {
            summary.real_agree += 1;
        }
        if game.theoretical == game.real {
            summary.theoretical_equal += 1;
        }
        if empty_count(replay.final_board) == 0 {
            summary.full_board.total += 1;
            summary.full_board.agree += usize::from(agree);
        } else {
            summary.empty_board.total += 1;
            summary.empty_board.agree += usize::from(agree);
            if game.real > 32 {
                summary.black_win.total += 1;
                summary.black_win.agree += usize::from(agree);
            } else if game.real < 32 {
                summary.black_loss.total += 1;
                summary.black_loss.agree += usize::from(agree);
            }
        }
    }
    summary
}

impl Summary {
    pub fn merge(&mut self, other: Summary) {
        self.games_read += other.games_read;
        self.replayed += other.replayed;
        self.rejected_illegal += other.rejected_illegal;
        self.rejected_incomplete += other.rejected_incomplete;
        self.real_agree += other.real_agree;
        self.theoretical_equal += other.theoretical_equal;
        self.full_board.merge(other.full_board);
        self.empty_board.merge(other.empty_board);
        self.black_win.merge(other.black_win);
        self.black_loss.merge(other.black_loss);
        for (p3, count) in other.p3_values {
            *self.p3_values.entry(p3).or_default() += count;
        }
    }
}

impl Group {
    fn merge(&mut self, other: Group) {
        self.agree += other.agree;
        self.total += other.total;
    }
}

pub fn run(dir: &Path, manifest_path: &Path) -> Result<Summary, String> {
    let manifest = std::fs::read_to_string(manifest_path)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    verify_manifest(dir, &manifest)?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("wtb"))
        .collect();
    files.sort();
    let mut summary = Summary::default();
    for path in files {
        summary.merge(summarise(&wthor::read_path(&path)?));
    }
    Ok(summary)
}

pub fn verify_file(path: &Path, expected: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let actual = digest(&bytes);
    if actual == expected.to_ascii_lowercase() {
        Ok(())
    } else {
        Err(format!("{}: SHA-256 mismatch", path.display()))
    }
}

pub fn verify_manifest(dir: &Path, manifest: &str) -> Result<(), String> {
    let mut listed = BTreeMap::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let hash = fields.next().ok_or("manifest: missing hash")?;
        let name = fields.next().ok_or("manifest: missing file name")?;
        let name = name.strip_prefix('*').unwrap_or(name);
        let name = Path::new(name)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("manifest: invalid file name")?;
        listed.insert(name.to_string(), hash.to_ascii_lowercase());
    }
    for (name, hash) in &listed {
        verify_file(&dir.join(name), hash)?;
    }
    for entry in std::fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))? {
        let path = entry
            .map_err(|error| format!("{}: {error}", dir.display()))?
            .path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("wtb") {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if !listed.contains_key(name) {
                return Err(format!("{}: not in manifest", path.display()));
            }
        }
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut text = String::with_capacity(64);
    for byte in hash {
        use std::fmt::Write;
        write!(&mut text, "{byte:02x}").unwrap();
    }
    text
}
