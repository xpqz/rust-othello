//! ProbCut v2 calibration (docs/design/probcut2.md § Calibration data).
//!
//! `collect`: unit `n` picks a WTHOR game and a root position in it, runs a
//! plain alpha-beta search of `SAMPLE_DEPTH` from the root with the engine's
//! move ordering, and keeps one node it visits with remaining depth >= 3,
//! chosen uniformly (reservoir). For that node it records the static value and
//! the full-width search value at every depth 1..=D, table cleared once per
//! node: D is `--depth`, or `--deep-depth` for every `--deep-every`th unit.
//! Units are deterministic in `n` and `--seed`; one journal line per unit, so
//! the job resumes by re-running the same command, and a larger `--nodes`
//! extends a finished run.
//!
//! usage: probcut_fit collect --out FILE [--nodes 12000] [--depth 14]
//!        [--deep-depth 16] [--deep-every 8] [--jobs 6] [--seed 145]

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use othello::board::{Board, empty_count, final_score, legal_moves, pass, play};
use othello::pattern::evaluate;
use othello::score::terminal_score;
use othello::search::{SearchConfig, SearchLimits, ordered_moves, search_tt};
use othello::tt::{MAX_ENTRIES, Table};
use othello_tools::journal::Journal;
use othello_tools::provenance::{file_hash, git_commit};

const WTHOR: &str = "data/wthor";
const SAMPLE_DEPTH: u32 = 8;
/// Root positions are drawn with this many empties, uniformly.
const ROOT_EMPTIES: std::ops::RangeInclusive<u32> = 26..=58;
/// Sampled nodes keep at least this many empties.
const MIN_EMPTIES: u32 = 16;
/// Games whose hashed index falls below this share (of 10) are the fit split.
const FIT_TENTHS: u64 = 7;

fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = mix(self.0);
        self.0 % n
    }
}

fn decode(byte: u8) -> Option<u8> {
    let (column, row) = (byte % 10, byte / 10);
    ((1..=8).contains(&column) && (1..=8).contains(&row)).then(|| (row - 1) * 8 + column - 1)
}

/// Every WTHOR game as its move list, files in name order. Returns the games
/// and a hash of the file list and sizes for the journal header.
fn wthor(root: &Path) -> Result<(Vec<Vec<u8>>, u64), String> {
    let dir = root.join(WTHOR);
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wtb"))
        })
        .collect();
    names.sort();
    let (mut games, mut hash) = (Vec::new(), 0u64);
    for name in names {
        let bytes = std::fs::read(&name).map_err(|error| error.to_string())?;
        hash = mix(hash ^ bytes.len() as u64 ^ file_hash(&name.to_string_lossy())?);
        for record in bytes.get(16..).unwrap_or_default().as_chunks::<68>().0 {
            let moves = record[8..].iter().take_while(|&&byte| byte != 0);
            games.push(moves.map(|&byte| decode(byte).unwrap_or(u8::MAX)).collect());
        }
    }
    Ok((games, hash))
}

/// The game's position with exactly `empties` empties and a legal move, if any.
fn position(moves: &[u8], empties: u32) -> Option<Board> {
    let mut board = Board::START;
    for &square in moves {
        if legal_moves(board.me, board.opp) == 0 {
            board = pass(board);
        }
        let movable = legal_moves(board.me, board.opp);
        if empty_count(board) == empties && movable != 0 {
            return Some(board);
        }
        if square >= 64 || movable & (1u64 << square) == 0 {
            return None;
        }
        board = play(board, square);
    }
    None
}

struct Sampler {
    rng: Rng,
    seen: u64,
    pick: Option<Board>,
}

fn order_eval(depth: u32) -> Option<fn(Board) -> i32> {
    (depth >= 4).then_some(evaluate as fn(Board) -> i32)
}

impl Sampler {
    fn search(&mut self, board: Board, depth: u32, mut alpha: i32, beta: i32) -> i32 {
        let moves = legal_moves(board.me, board.opp);
        if moves == 0 {
            if legal_moves(board.opp, board.me) == 0 {
                return terminal_score(final_score(board));
            }
            return -self.search(pass(board), depth, -beta, -alpha);
        }
        if depth == 0 {
            return evaluate(board);
        }
        if depth >= 3 && empty_count(board) >= MIN_EMPTIES {
            self.seen += 1;
            if self.rng.below(self.seen) == 0 {
                self.pick = Some(board);
            }
        }
        let (order, len) = ordered_moves(board, moves, 0, None, order_eval(depth));
        let mut best = i32::MIN;
        for &square in &order[..len] {
            let value = -self.search(play(board, square), depth - 1, -beta, -alpha);
            best = best.max(value);
            alpha = alpha.max(value);
            if alpha >= beta {
                break;
            }
        }
        best
    }
}

/// Unit `n`: (game index, sampled node). Redraws until a game has a root.
fn unit(games: &[Vec<u8>], seed: u64, n: usize) -> (usize, Board) {
    let mut rng = Rng(mix(seed ^ mix(n as u64)));
    loop {
        let game = rng.below(games.len() as u64) as usize;
        let span = u64::from(ROOT_EMPTIES.end() - ROOT_EMPTIES.start() + 1);
        let empties = ROOT_EMPTIES.start() + rng.below(span) as u32;
        let Some(root) = position(&games[game], empties) else {
            continue;
        };
        let mut sampler = Sampler {
            rng: Rng(rng.0),
            seen: 0,
            pick: None,
        };
        sampler.search(root, SAMPLE_DEPTH, -i32::MAX, i32::MAX);
        if let Some(node) = sampler.pick {
            return (game, node);
        }
        rng.0 = sampler.rng.0;
    }
}

fn measure(node: Board, depth: u32, table: &mut Table) -> (Vec<i32>, u64) {
    let config = SearchConfig {
        endgame_threshold: 0,
        ..SearchConfig::standard()
    };
    table.clear();
    let (mut values, mut nodes) = (vec![evaluate(node)], 0);
    for d in 1..=depth {
        let result = search_tt(node, SearchLimits::depth(d), &config, table);
        values.push(result.score);
        nodes += result.nodes;
    }
    (values, nodes)
}

fn collect(args: &[String]) -> Result<(), String> {
    let value = |flag: &str, default: &str| {
        let at = args.iter().position(|arg| arg == flag);
        at.and_then(|at| args.get(at + 1))
            .map_or(default.to_string(), String::clone)
    };
    let number = |flag: &str, default: &str| {
        value(flag, default)
            .parse::<u64>()
            .map_err(|_| format!("invalid {flag}"))
    };
    let out = value("--out", "");
    if out.is_empty() {
        return Err("usage: probcut_fit collect --out FILE [--nodes N] [--depth D] [--deep-depth D] [--deep-every K] [--jobs J] [--seed S]".into());
    }
    let nodes = number("--nodes", "12000")? as usize;
    let depth = number("--depth", "14")? as u32;
    let deep = number("--deep-depth", "16")? as u32;
    let every = number("--deep-every", "8")?.max(1) as usize;
    let jobs = number("--jobs", "6")?.max(1) as usize;
    let seed = number("--seed", "145")?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let (games, data) = wthor(&root)?;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    // `--nodes` and `--jobs` are left out: they change neither unit.
    let header = format!(
        "header v1 probcut_fit collect depth {depth} deep_depth {deep} deep_every {every} seed {seed} sample_depth {SAMPLE_DEPTH} root_empties {}..={} min_empties {MIN_EMPTIES} games {} data {data:016x} commit {} binary {:016x}",
        ROOT_EMPTIES.start(),
        ROOT_EMPTIES.end(),
        games.len(),
        git_commit(),
        file_hash(&exe.to_string_lossy())?
    );
    let (journal, done) = Journal::open(Path::new(&out), &header)?;
    let pending: Vec<usize> = (0..nodes)
        .filter(|n| !done.contains(&n.to_string()))
        .collect();
    let (journal, next, finished) = (
        Mutex::new(journal),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    );
    let start = Instant::now();
    eprintln!(
        "probcut_fit collect: {} of {nodes} units to do, {jobs} jobs",
        pending.len()
    );
    std::thread::scope(|scope| -> Result<(), String> {
        let workers: Vec<_> = (0..jobs)
            .map(|_| {
                scope.spawn(|| -> Result<(), String> {
                    let mut table = Table::new(MAX_ENTRIES);
                    while let Some(&n) = pending.get(next.fetch_add(1, Relaxed)) {
                        let (game, node) = unit(&games, seed, n);
                        let max = if n % every == 0 { deep } else { depth };
                        let begin = Instant::now();
                        let (values, searched) = measure(node, max, &mut table);
                        let split = if mix(game as u64 ^ seed) % 10 < FIT_TENTHS { "fit" } else { "held" };
                        let list: Vec<String> = values.iter().map(i32::to_string).collect();
                        let line = format!(
                            "game {game} split {split} empties {} board {:016x}:{:016x} depth {max} nodes {searched} ms {:.0} values {}",
                            empty_count(node),
                            node.me,
                            node.opp,
                            begin.elapsed().as_secs_f64() * 1000.0,
                            list.join(",")
                        );
                        journal.lock().unwrap().append(&n.to_string(), &line)?;
                        let count = finished.fetch_add(1, Relaxed) + 1;
                        if count % 50 == 0 || count == pending.len() {
                            let elapsed = start.elapsed().as_secs_f64();
                            let eta = elapsed / count as f64 * (pending.len() - count) as f64;
                            eprintln!(
                                "progress {count}/{} elapsed {elapsed:.0}s eta {eta:.0}s",
                                pending.len()
                            );
                        }
                    }
                    Ok(())
                })
            })
            .collect();
        workers
            .into_iter()
            .try_for_each(|worker| worker.join().map_err(|_| "worker panicked".to_string())?)
    })?;
    let journal = journal.into_inner().unwrap();
    let records = journal.records();
    let ms: f64 = records
        .iter()
        .filter_map(|record| {
            record
                .line
                .split(" ms ")
                .nth(1)?
                .split(' ')
                .next()?
                .parse::<f64>()
                .ok()
        })
        .sum();
    println!(
        "probcut_fit collect units {} cpu_s {:.0} wall_s {:.0}",
        records.len(),
        ms / 1000.0,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("collect") => collect(&args[1..]),
        _ => Err("usage: probcut_fit collect --out FILE ...".into()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}
