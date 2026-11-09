use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    pub line: String,
}

pub struct Journal {
    path: PathBuf,
    header: String,
    file: File,
    records: Vec<Record>,
}

fn valid_prefix(text: &str) -> &str {
    if text.ends_with('\n') {
        text
    } else {
        &text[..text.rfind('\n').map_or(0, |index| index + 1)]
    }
}

fn parse(text: &str) -> Result<(String, Vec<Record>), String> {
    let mut lines = valid_prefix(text).lines();
    let header = lines.next().unwrap_or("").to_string();
    let mut records = Vec::new();
    for line in lines {
        let (id, payload) = line
            .split_once('\t')
            .ok_or_else(|| format!("malformed journal line: {line}"))?;
        if id.is_empty() || payload.is_empty() {
            return Err(format!("malformed journal line: {line}"));
        }
        records.push(Record {
            id: id.into(),
            line: payload.into(),
        });
    }
    Ok((header, records))
}

pub fn read(path: &Path) -> Result<(String, Vec<Record>), String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    parse(&text)
}

pub fn mismatch(expected: &str, found: &str) -> String {
    format!("journal header mismatch:\n expected {expected}\n found {found}")
}

fn rewrite(path: &Path, text: &str) -> Result<(), String> {
    let temporary = PathBuf::from(format!("{}.tmp", path.display()));
    let mut file = File::create(&temporary).map_err(|error| error.to_string())?;
    file.write_all(text.as_bytes())
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

impl Journal {
    pub fn open(path: &Path, header: &str) -> Result<(Self, BTreeSet<String>), String> {
        let mut records = Vec::new();
        if path.exists() {
            let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
            let valid = valid_prefix(&text);
            if valid.len() != text.len() {
                rewrite(path, valid)?;
            }
            let (found, parsed) = parse(&text)?;
            records = parsed;
            if found.is_empty() {
                rewrite(path, &format!("{header}\n"))?;
            } else if found != header {
                return Err(mismatch(header, &found));
            }
        } else {
            rewrite(path, &format!("{header}\n"))?;
        }
        let file = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        let done = records.iter().map(|record| record.id.clone()).collect();
        Ok((
            Self {
                path: path.into(),
                header: header.into(),
                file,
                records,
            },
            done,
        ))
    }

    pub fn append(&mut self, id: &str, line: &str) -> Result<(), String> {
        if id.is_empty() || id.contains(['\t', '\n']) || line.contains(['\t', '\n']) {
            return Err("invalid journal unit".into());
        }
        writeln!(self.file, "{id}\t{line}").map_err(|error| error.to_string())?;
        self.file.flush().map_err(|error| error.to_string())?;
        self.records.push(Record {
            id: id.into(),
            line: line.into(),
        });
        Ok(())
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    pub fn discard(&mut self, ids: &BTreeSet<String>) -> Result<(), String> {
        self.records.retain(|record| !ids.contains(&record.id));
        let mut text = format!("{}\n", self.header);
        for record in &self.records {
            text.push_str(&format!("{}\t{}\n", record.id, record.line));
        }
        rewrite(&self.path, &text)?;
        self.file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}
