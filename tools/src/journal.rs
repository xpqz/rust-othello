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
    let text = valid_prefix(text);
    let mut lines = text.lines();
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

impl Journal {
    pub fn open(path: &Path, header: &str) -> Result<(Self, BTreeSet<String>), String> {
        if path.exists() {
            let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
            let valid = valid_prefix(&text);
            if valid.len() != text.len() {
                fs::write(path, valid).expect("repair journal");
            }
            let (found, records) = parse(&text)?;
            if found.is_empty() {
                fs::write(path, format!("{header}\n")).expect("write journal");
            } else if found != header {
                return Err(format!(
                    "journal header mismatch:\n expected {header}\n found {found}"
                ));
            }
            let file = OpenOptions::new()
                .append(true)
                .open(path)
                .expect("open journal");
            let done = records.iter().map(|record| record.id.clone()).collect();
            return Ok((
                Self {
                    path: path.into(),
                    header: header.into(),
                    file,
                    records,
                },
                done,
            ));
        }
        let mut file = File::create(path).expect("create journal");
        writeln!(file, "{header}").expect("write header");
        file.flush().expect("flush header");
        let file = OpenOptions::new()
            .append(true)
            .open(path)
            .expect("open journal");
        Ok((
            Self {
                path: path.into(),
                header: header.into(),
                file,
                records: Vec::new(),
            },
            BTreeSet::new(),
        ))
    }

    pub fn append(&mut self, id: &str, line: &str) -> Result<(), String> {
        if id.is_empty() || id.contains(['\t', '\n']) || line.contains(['\t', '\n']) {
            return Err("invalid journal unit".into());
        }
        writeln!(self.file, "{id}\t{line}").expect("append journal");
        self.file.flush().expect("flush journal");
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
        let mut file = File::create(&self.path).expect("rewrite journal");
        writeln!(file, "{}", self.header).expect("write header");
        for record in &self.records {
            writeln!(file, "{}\t{}", record.id, record.line).expect("write record");
        }
        file.flush().expect("flush journal");
        self.file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .expect("open journal");
        Ok(())
    }
}
