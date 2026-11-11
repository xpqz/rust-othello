use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use othello::board::{Board, empty_count};
use othello::score::INF;
use othello::search::{self, Ctx, SearchConfig};
use othello_tools::journal::Journal;
use othello_tools::provenance::git_commit;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use crate::check;
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
        Some((board, black)) => {
            let value = solve(board)? / 100;
            Some(side(value, black))
        }
        None => None,
    };
    let theoretical = 2 * i32::from(game.theoretical) - 64;
    let final_value = 2 * i32::from(game.real) - 64;
    let mut out = Vec::with_capacity(replay.positions.len());
    for &(board, black) in &replay.positions {
        let empties = empty_count(board);
        let label = if empties >= u32::from(p3) {
            side(theoretical, black)
        } else if empties >= 15 {
            side(anchor_value.unwrap_or(final_value), black)
        } else if anchor.is_some_and(|(anchor_board, _)| board == anchor_board) {
            side(anchor_value.unwrap(), black)
        } else {
            solve(board)? / 100
        };
        out.push(Labelled {
            board,
            black,
            label,
        });
    }
    Ok(out)
}

fn side(value: i32, black: bool) -> i32 {
    if black { value } else { -value }
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
    let (header, mut tasks) = inputs(dir, limit)?;
    let (journal, done) = Journal::open(path, &header)?;
    tasks.retain(|job| !done.contains(&job.key));
    let journal = Mutex::new(journal);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .map_err(|error| error.to_string())?;
    let games = AtomicUsize::new(0);
    let positions = AtomicUsize::new(0);
    let rejected = AtomicUsize::new(0);
    pool.install(|| {
        tasks.par_iter().try_for_each(|job| -> Result<(), String> {
            let replay = match wthor::replay(&job.game) {
                Ok(replay) => replay,
                Err(_) => {
                    rejected.fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
            };
            let config = SearchConfig::standard();
            let mut ctx = Ctx::new(&config);
            let mut solver =
                |board| search::solve(board, -INF, INF, &mut ctx).ok_or("solve failed".to_string());
            let values = labels(job.p3, &job.game, &replay, &mut solver)?;
            let line = encode(&values);
            journal.lock().unwrap().append(&job.key, &line)?;
            games.fetch_add(1, Ordering::Relaxed);
            positions.fetch_add(values.len(), Ordering::Relaxed);
            Ok(())
        })
    })?;
    println!(
        "games {} positions {} rejected {} elapsed {:.2}s",
        games.load(Ordering::Relaxed),
        positions.load(Ordering::Relaxed),
        rejected.load(Ordering::Relaxed),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn inputs(dir: &Path, limit: Option<usize>) -> Result<(String, Vec<Job>), String> {
    let files = check::wtb_files(dir)?;
    let mut hasher = Sha256::new();
    let mut p3s = BTreeSet::new();
    let mut jobs = Vec::new();
    let mut seen = 0;
    for path in files {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid input file name")?
            .to_string();
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        hasher.update(name.as_bytes());
        hasher.update(&bytes);
        let (p3, records) = wthor::read(&bytes)?;
        p3s.insert(p3);
        for (index, game) in records.into_iter().enumerate() {
            if limit.is_some_and(|limit| seen >= limit) {
                break;
            }
            seen += 1;
            jobs.push(Job {
                key: format!("{name}:{index}"),
                p3,
                game,
            });
        }
    }
    let p3s: Vec<String> = p3s.iter().map(u8::to_string).collect();
    let header = format!(
        "header v1 engine {} input {} p3 {} threshold 14",
        git_commit(),
        check::digest(hasher.finalize().as_slice()),
        p3s.join(",")
    );
    Ok((header, jobs))
}
