use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use othello::board::{Board, empty_count};
use othello::score::INF;
use othello::search::{self, Ctx, SearchConfig};
use othello_tools::journal::Journal;
use othello_tools::provenance::{file_hash, git_commit};

use crate::wthor::{self, Game, Replay};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Labelled {
    pub board: Board,
    pub black: bool,
    pub label: i32,
}

struct Job {
    key: String,
    p3: u8,
    game: Game,
}

pub fn labels(
    p3: u8,
    game: &Game,
    replay: &Replay,
    solve: &mut dyn FnMut(Board) -> Result<i32, String>,
) -> Result<Vec<Labelled>, String> {
    let anchor = replay
        .positions
        .iter()
        .find(|(board, _)| empty_count(*board) <= 14)
        .copied();
    let anchor_value = match anchor {
        Some((board, _)) => Some(solve(board)?),
        None => None,
    };
    let theoretical = 2 * i32::from(game.theoretical) - 64;
    let final_value = 2 * i32::from(game.real) - 64;
    let mut out = Vec::with_capacity(replay.positions.len());
    for &(board, black) in &replay.positions {
        let empties = empty_count(board);
        let black_value = if empties >= u32::from(p3) {
            theoretical
        } else if empties >= 15 {
            match (anchor, anchor_value) {
                (Some((_, anchor_black)), Some(centidiscs)) => {
                    let value = centidiscs / 100;
                    if anchor_black { value } else { -value }
                }
                _ => final_value,
            }
        } else {
            match (anchor, anchor_value) {
                (Some((anchor_board, anchor_black)), Some(centidiscs)) if board == anchor_board => {
                    let value = centidiscs / 100;
                    if anchor_black { value } else { -value }
                }
                _ => solve(board)? / 100,
            }
        };
        out.push(Labelled {
            board,
            black,
            label: if black { black_value } else { -black_value },
        });
    }
    Ok(out)
}

pub fn encode(labels: &[Labelled]) -> String {
    labels
        .iter()
        .map(|label| label.label.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn decode(line: &str) -> Result<Vec<i32>, String> {
    line.split_whitespace()
        .map(|part| part.parse().map_err(|_| format!("invalid label {part}")))
        .collect()
}

pub fn run(dir: &Path, path: &Path, jobs: usize, limit: Option<usize>) -> Result<(), String> {
    let start = Instant::now();
    let inputs = inputs(dir)?;
    let header = header(&inputs)?;
    let (journal, done) = Journal::open(path, &header)?;
    let tasks = tasks(inputs, &done, limit)?;
    let journal = Mutex::new(journal);
    let next = AtomicUsize::new(0);
    let games = AtomicUsize::new(0);
    let positions = AtomicUsize::new(0);
    let rejected = AtomicUsize::new(0);
    let errors = Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = tasks.get(index) else {
                        break;
                    };
                    let replay = match wthor::replay(&job.game) {
                        Ok(replay) => replay,
                        Err(_) => {
                            rejected.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    };
                    let config = SearchConfig::standard();
                    let mut ctx = Ctx::new(&config);
                    let mut solver = |board| {
                        search::solve(board, -INF, INF, &mut ctx).ok_or("solve failed".into())
                    };
                    match labels(job.p3, &job.game, &replay, &mut solver) {
                        Ok(values) => {
                            let line = encode(&values);
                            if let Err(error) = journal.lock().unwrap().append(&job.key, &line) {
                                errors.lock().unwrap().push(error);
                                return;
                            }
                            games.fetch_add(1, Ordering::Relaxed);
                            positions.fetch_add(values.len(), Ordering::Relaxed);
                        }
                        Err(error) => {
                            errors.lock().unwrap().push(error);
                            return;
                        }
                    }
                }
            });
        }
    });
    if let Some(error) = errors.lock().unwrap().first() {
        return Err(error.clone());
    }
    println!(
        "games {} positions {} rejected {} elapsed {:.2}s",
        games.load(Ordering::Relaxed),
        positions.load(Ordering::Relaxed),
        rejected.load(Ordering::Relaxed),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn inputs(dir: &Path) -> Result<Vec<(PathBuf, Vec<u8>, u8)>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("wtb"))
        .collect();
    files.sort();
    let mut inputs = Vec::with_capacity(files.len());
    for path in files {
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if bytes.len() < wthor::HEADER {
            return Err(format!("{}: file shorter than header", path.display()));
        }
        let p3 = if bytes[14] == 0 { 22 } else { bytes[14] };
        inputs.push((path, bytes, p3));
    }
    Ok(inputs)
}

fn header(inputs: &[(PathBuf, Vec<u8>, u8)]) -> Result<String, String> {
    let mut input_hash = 0xcbf2_9ce4_8422_2325u64;
    let mut p3s = BTreeSet::new();
    for (path, _, p3) in inputs {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid input file name")?;
        for byte in name.bytes() {
            input_hash ^= u64::from(byte);
            input_hash = input_hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let path = path.to_str().ok_or("invalid input path")?;
        input_hash ^= file_hash(path)?;
        input_hash = input_hash.wrapping_mul(0x0000_0100_0000_01b3);
        p3s.insert(*p3);
    }
    let p3s: Vec<String> = p3s.iter().map(u8::to_string).collect();
    Ok(format!(
        "header v1 engine {} input {:016x} p3 {} threshold 14",
        git_commit(),
        input_hash,
        p3s.join(",")
    ))
}

fn tasks(
    inputs: Vec<(PathBuf, Vec<u8>, u8)>,
    done: &BTreeSet<String>,
    limit: Option<usize>,
) -> Result<Vec<Job>, String> {
    let mut tasks = Vec::new();
    let mut seen = 0;
    for (path, bytes, p3) in inputs {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid input file name")?
            .to_string();
        let (_, records) = wthor::read(&bytes)?;
        for (index, game) in records.into_iter().enumerate() {
            if limit.is_some_and(|limit| seen >= limit) {
                return Ok(tasks);
            }
            seen += 1;
            let key = format!("{name}:{index}");
            if !done.contains(&key) {
                tasks.push(Job { key, p3, game });
            }
        }
    }
    Ok(tasks)
}
