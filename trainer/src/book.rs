//! Opening book builder (`docs/design/book.md`). Nodes are canonical,
//! normalized positions; values are centidiscs from the side to move.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use othello::board::{Board, final_score, is_terminal, legal_moves, pass, play, squares};
use othello::pattern;
use othello::rng::Rng;
use othello::score::terminal_score;
use othello::search::{ProbCut, SearchConfig, SearchLimits, search_tt};
use othello::tt::{Table, zobrist};
use othello_tools::journal::{self, Journal};
use othello_tools::provenance::git_commit;
use othello_tools::sym::canonical;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use crate::check;
use crate::wthor;

pub type Key = (u64, u64);
type Search<'a> = dyn FnMut(Board, u32) -> i32 + 'a;
/// Drop-out results by node: (value, square), or none.
pub type Drops = BTreeMap<Key, Option<(i32, u8)>>;

const RECORD: usize = 11;
const RESAMPLES: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub square: u8,
    pub child: Key,
    pub passed: bool,
    pub book: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub value: i32,
    /// `None` only for a terminal node.
    pub square: Option<u8>,
    pub book: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub min: u32,
    pub plies: u32,
    pub depth: u32,
    pub table: usize,
}

pub fn key(board: Board) -> Key {
    canonical(board)
}

pub fn board(key: Key) -> Board {
    Board {
        me: key.0,
        opp: key.1,
    }
}

/// Replaces a board whose mover must pass by the passed board.
pub fn normalize(board: Board) -> (Board, bool) {
    let stuck = legal_moves(board.me, board.opp) == 0;
    if stuck && legal_moves(board.opp, board.me) != 0 {
        (pass(board), true)
    } else {
        (board, false)
    }
}

/// Positions after 0..`plies` - 1 placements, each counted once per game.
pub fn count(games: &[Vec<u8>], plies: u32) -> BTreeMap<Key, u32> {
    let mut counts = BTreeMap::new();
    for game in games {
        let seen: BTreeSet<Key> = line(game)
            .into_iter()
            .take(plies as usize)
            .map(key)
            .collect();
        for k in seen {
            *counts.entry(k).or_default() += 1;
        }
    }
    counts
}

/// The normalized board after each number of placements, stopping at the
/// first illegal move.
fn line(moves: &[u8]) -> Vec<Board> {
    let mut boards = vec![Board::START];
    for &square in moves {
        let current = boards[boards.len() - 1];
        if square >= 64 || legal_moves(current.me, current.opp) & (1 << square) == 0 {
            break;
        }
        boards.push(normalize(play(current, square)).0);
    }
    boards
}

fn moves_where(key: Key, is_node: impl Fn(&Key) -> bool) -> Vec<Move> {
    let node = board(key);
    squares(legal_moves(node.me, node.opp))
        .map(|square| {
            let (child, passed) = normalize(play(node, square));
            let child = self::key(child);
            Move {
                square,
                child,
                passed,
                book: is_node(&child),
            }
        })
        .collect()
}

/// Every legal move of the node, in ascending square order.
pub fn partition(key: Key, nodes: &BTreeMap<Key, u32>) -> Vec<Move> {
    moves_where(key, |child| nodes.contains_key(child))
}

/// A board's value at `depth`: exact if terminal, passing at the same depth,
/// otherwise `search`, which only ever sees a board with a legal move.
pub fn value(board: Board, depth: u32, search: &mut Search) -> i32 {
    if is_terminal(board) {
        terminal_score(final_score(board))
    } else if legal_moves(board.me, board.opp) == 0 {
        -value(pass(board), depth, search)
    } else {
        search(board, depth)
    }
}

/// The best drop-out move, each searched at `depth` - 1 in square order.
pub fn dropout(
    key: Key,
    nodes: &BTreeMap<Key, u32>,
    depth: u32,
    search: &mut Search,
) -> Option<(i32, u8)> {
    let node = board(key);
    let mut best: Option<(i32, u8)> = None;
    for m in partition(key, nodes).into_iter().filter(|m| !m.book) {
        let found = -value(play(node, m.square), depth - 1, search);
        if best.is_none_or(|(value, _)| found > value) {
            best = Some((found, m.square));
        }
    }
    best
}

/// Negamax over the node DAG, children (more discs) first.
pub fn backup(nodes: &BTreeMap<Key, u32>, drops: &Drops) -> Result<BTreeMap<Key, Entry>, String> {
    let mut order: Vec<Key> = nodes.keys().copied().collect();
    order.sort_by_key(|&(me, opp)| std::cmp::Reverse((me | opp).count_ones()));
    let mut entries = BTreeMap::new();
    for k in order {
        let node = board(k);
        if is_terminal(node) {
            let value = terminal_score(final_score(node));
            entries.insert(
                k,
                Entry {
                    value,
                    square: None,
                    book: false,
                },
            );
            continue;
        }
        let drop = drops
            .get(&k)
            .ok_or_else(|| format!("no drop-out result for {}", id(k)))?;
        let mut best = drop.map(|(value, square)| Entry {
            value,
            square: Some(square),
            book: false,
        });
        for m in partition(k, nodes).into_iter().filter(|m| m.book) {
            let child = entries[&m.child].value;
            let value = if m.passed { child } else { -child };
            let better = best.is_none_or(|e: Entry| {
                value > e.value || (value == e.value && Some(m.square) < e.square)
            });
            if better {
                best = Some(Entry {
                    value,
                    square: Some(m.square),
                    book: true,
                });
            }
        }
        entries.insert(k, best.ok_or("node without moves")?);
    }
    Ok(entries)
}

/// Book moves of every node, computed once for repeated walks.
type Graph = BTreeMap<Key, Vec<Move>>;

fn graph(entries: &BTreeMap<Key, Entry>) -> Graph {
    entries
        .keys()
        .map(|&k| {
            let moves = moves_where(k, |child| entries.contains_key(child));
            (k, moves.into_iter().filter(|m| m.book).collect())
        })
        .collect()
}

fn walk(
    roots: &[(Key, bool)],
    entries: &BTreeMap<Key, Entry>,
    graph: &Graph,
    removed: &BTreeSet<Key>,
) -> BTreeSet<Key> {
    let (mut records, mut seen) = (BTreeSet::new(), BTreeSet::new());
    let mut stack = roots.to_vec();
    while let Some((k, engine)) = stack.pop() {
        let Some(entry) = entries.get(&k) else {
            continue;
        };
        if entry.square.is_none() || !seen.insert((k, engine)) {
            continue;
        }
        let next = |m: &Move| (m.child, if m.passed { engine } else { !engine });
        if !engine {
            stack.extend(graph[&k].iter().map(next));
        } else if !removed.contains(&k) {
            records.insert(k);
            if entry.book {
                stack.extend(
                    graph[&k]
                        .iter()
                        .filter(|m| Some(m.square) == entry.square)
                        .map(next),
                );
            }
        }
    }
    records
}

/// Engine-to-move records reached from the roots, where the engine follows
/// its book move and the opponent any book move.
pub fn reachable(
    roots: &[(Key, bool)],
    entries: &BTreeMap<Key, Entry>,
    removed: &BTreeSet<Key>,
) -> BTreeSet<Key> {
    walk(roots, entries, &graph(entries), removed)
}

/// Removes records in ascending (count, zobrist) order, recomputing
/// reachability, until at most `room` remain. Mandatory keys are walk roots
/// with the engine to move, so no removal can disconnect them.
pub fn truncate(
    roots: &[(Key, bool)],
    entries: &BTreeMap<Key, Entry>,
    counts: &BTreeMap<Key, u32>,
    mandatory: &BTreeSet<Key>,
    room: usize,
) -> Result<BTreeSet<Key>, String> {
    let graph = graph(entries);
    let mut roots = roots.to_vec();
    roots.extend(mandatory.iter().map(|&k| (k, true)));
    let mut removed = BTreeSet::new();
    loop {
        let records = walk(&roots, entries, &graph, &removed);
        if let Some(k) = mandatory.iter().find(|k| !records.contains(k)) {
            return Err(format!("mandatory {} is not a movable book node", id(*k)));
        }
        if records.len() <= room {
            return Ok(records);
        }
        let victim = records
            .iter()
            .filter(|k| !mandatory.contains(k))
            .min_by_key(|&&k| (counts.get(&k).copied().unwrap_or(0), zobrist(board(k))))
            .ok_or_else(|| format!("{} mandatory records exceed room {room}", records.len()))?;
        removed.insert(*victim);
    }
}

/// Sorted 11-byte little-endian records: key, square, value as i16.
pub fn encode(records: &[(u64, u8, i32)]) -> Result<Vec<u8>, String> {
    let mut sorted = records.to_vec();
    sorted.sort_by_key(|record| record.0);
    let mut bytes = Vec::with_capacity(sorted.len() * RECORD);
    for (index, &(k, square, value)) in sorted.iter().enumerate() {
        if square >= 64 {
            return Err(format!("square {square} out of range"));
        }
        if index > 0 && sorted[index - 1].0 == k {
            return Err(format!("duplicate key {k:016x}"));
        }
        bytes.extend_from_slice(&k.to_le_bytes());
        bytes.push(square);
        bytes.extend_from_slice(&clamp(value).to_le_bytes());
    }
    Ok(bytes)
}

fn clamp(value: i32) -> i16 {
    value.clamp(i16::MIN.into(), i16::MAX.into()) as i16
}

/// Fisher–Yates, i falling, j = `next_u64()` % (i + 1).
pub fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut rng = Rng::new(seed);
    for i in (1..items.len()).rev() {
        items.swap(i, (rng.next_u64() % (i as u64 + 1)) as usize);
    }
}

/// `n` gate roots: movable nodes at the given placements, each as the first
/// game's prefix reaching it, drawn by the seeded shuffle of the sorted pool.
pub fn roots(
    games: &[Vec<u8>],
    nodes: &BTreeMap<Key, u32>,
    plies: RangeInclusive<usize>,
    n: usize,
    seed: u64,
) -> Result<Vec<Vec<u8>>, String> {
    let mut pool: BTreeMap<Key, Vec<u8>> = BTreeMap::new();
    for game in games {
        for (at, board) in line(game)
            .into_iter()
            .enumerate()
            .filter(|(at, _)| plies.contains(at))
        {
            let k = key(board);
            if nodes.contains_key(&k) && legal_moves(board.me, board.opp) != 0 {
                pool.entry(k).or_insert_with(|| game[..at].to_vec());
            }
        }
    }
    if n > pool.len() {
        return Err(format!("{n} roots requested from a pool of {}", pool.len()));
    }
    let mut lines: Vec<Vec<u8>> = pool.into_values().collect();
    shuffle(&mut lines, seed);
    lines.truncate(n);
    Ok(lines)
}

/// Projected hours on `threads`: the mean, and the 97.5th percentile
/// (nearest rank) of 10,000 bootstrap resamples.
pub fn projection(times: &[f64], nodes: usize, threads: usize, seed: u64) -> (f64, f64) {
    let scale = |mean: f64| mean * nodes as f64 / threads as f64 / 3600.0;
    let n = times.len();
    let mut rng = Rng::new(seed);
    let mut draws: Vec<f64> = (0..RESAMPLES)
        .map(|_| {
            let sum: f64 = (0..n)
                .map(|_| times[(rng.next_u64() % n as u64) as usize])
                .sum();
            scale(sum / n as f64)
        })
        .collect();
    draws.sort_by(f64::total_cmp);
    let rank = (0.975 * RESAMPLES as f64).ceil() as usize - 1;
    (scale(times.iter().sum::<f64>() / n as f64), draws[rank])
}

/// The largest depth whose upper bound is within `limit` hours.
pub fn depth_rule(bounds: &[(u32, f64)], limit: f64) -> Option<u32> {
    bounds
        .iter()
        .filter(|&&(_, upper)| upper <= limit)
        .map(|&(depth, _)| depth)
        .max()
}

/// The scoring policy, field by field.
pub fn policy() -> SearchConfig {
    SearchConfig {
        eval: pattern::evaluate,
        endgame_threshold: 18,
        ordering: true,
        probcut: Some(ProbCut::FITTED),
    }
}

pub fn header(params: &Params, input: &str) -> String {
    let config = policy();
    let eval = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/eval.bin"))
        .map_or_else(
            |_| "missing".into(),
            |bytes| check::hex(&Sha256::digest(bytes)),
        );
    format!(
        "book v1 min {} plies {} depth {} table {} input {input} eval {eval} engine {} \
         threshold {} ordering {} probcut {:?} rules normalize-passes,ascending-squares,\
         clear-per-node,lower-square-ties",
        params.min,
        params.plies,
        params.depth,
        params.table,
        git_commit(),
        config.endgame_threshold,
        config.ordering,
        config.probcut,
    )
}

fn id(k: Key) -> String {
    format!("{:016x}:{:016x}", k.0, k.1)
}

fn parse_id(text: &str) -> Result<Key, String> {
    let (me, opp) = text.split_once(':').ok_or("invalid node id")?;
    let hex = |part| u64::from_str_radix(part, 16).map_err(|_| format!("invalid node id {text}"));
    Ok((hex(me)?, hex(opp)?))
}

/// Runs each unit not yet in the journal on `jobs` threads, clearing the
/// table before each unit. Returns the number of units already done.
fn run_units<U: Sync>(
    units: &[(String, U)],
    params: &Params,
    jobs: usize,
    path: &Path,
    header: &str,
    unit: impl Fn(&U, &mut Search) -> String + Sync,
) -> Result<usize, String> {
    let (journal, done) = Journal::open(path, header)?;
    let todo: Vec<_> = units.iter().filter(|(id, _)| !done.contains(id)).collect();
    let journal = Mutex::new(journal);
    let config = policy();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .map_err(|error| error.to_string())?;
    pool.install(|| {
        todo.par_iter().try_for_each_init(
            || Table::new(params.table),
            |table, (id, item)| {
                table.clear();
                let mut search =
                    |b: Board, d: u32| search_tt(b, SearchLimits::depth(d), &config, table).score;
                let line = unit(item, &mut search);
                journal.lock().unwrap().append(id, &line)
            },
        )
    })?;
    Ok(units.len() - todo.len())
}

/// Scores every node not yet in the journal: one line per node, the
/// drop-out result and each drop-out search's score. Returns (done, scored).
pub fn score(
    nodes: &[Key],
    params: &Params,
    jobs: usize,
    path: &Path,
    header: &str,
) -> Result<(usize, usize), String> {
    let set: BTreeMap<Key, u32> = nodes.iter().map(|&k| (k, 0)).collect();
    let units: Vec<(String, Key)> = set.keys().map(|&k| (id(k), k)).collect();
    let done = run_units(&units, params, jobs, path, header, |&k, search| {
        let mut scores = Vec::new();
        let mut recorded = |b: Board, d: u32| {
            let found = search(b, d);
            scores.push(found.to_string());
            found
        };
        let result = match dropout(k, &set, params.depth, &mut recorded) {
            Some((value, square)) => format!("{value} {square}"),
            None => "none".into(),
        };
        format!("{result} | {}", scores.join(" "))
    })?;
    Ok((done, units.len() - done))
}

fn read_drops(path: &Path) -> Result<(String, Drops), String> {
    let (header, records) = journal::read(path)?;
    let mut drops = BTreeMap::new();
    for record in records {
        let result = record.line.split(" | ").next().unwrap_or_default();
        let drop = match result.split_once(' ') {
            None if result == "none" => None,
            Some((value, square)) => Some((
                value
                    .parse()
                    .map_err(|_| format!("invalid value in {}", record.id))?,
                square
                    .parse()
                    .map_err(|_| format!("invalid square in {}", record.id))?,
            )),
            None => return Err(format!("invalid record {}", record.id)),
        };
        drops.insert(parse_id(&record.id)?, drop);
    }
    Ok((header, drops))
}

/// Backs up the journal, prunes to the roots and the room, and returns the
/// asset and its manifest, both in asset order.
pub fn build(
    nodes: &BTreeMap<Key, u32>,
    path: &Path,
    roots: &[(Key, bool)],
    mandatory: &BTreeSet<Key>,
    room: usize,
) -> Result<(Vec<u8>, String), String> {
    let (header, drops) = read_drops(path)?;
    let entries = backup(nodes, &drops)?;
    let kept = truncate(roots, &entries, nodes, mandatory, room / RECORD)?;
    let mut rows: Vec<(u64, Key, u8, i32)> = kept
        .iter()
        .map(|&k| {
            (
                zobrist(board(k)),
                k,
                entries[&k].square.unwrap(),
                entries[&k].value,
            )
        })
        .collect();
    rows.sort_by_key(|row| row.0);
    let asset = encode(
        &rows
            .iter()
            .map(|&(z, _, s, v)| (z, s, v))
            .collect::<Vec<_>>(),
    )?;
    let mut manifest = format!(
        "# {header}\n# nodes {} records {}\n",
        nodes.len(),
        rows.len()
    );
    for (_, k, square, value) in rows {
        manifest.push_str(&format!("{} {square} {}\n", id(k), clamp(value)));
    }
    Ok((asset, manifest))
}

/// Every game's moves in file order, and a hash of the file names and bytes.
pub fn read_games(dir: &Path) -> Result<(Vec<Vec<u8>>, String), String> {
    let mut hasher = Sha256::new();
    let mut games = Vec::new();
    for path in check::wtb_files(dir)? {
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        hasher.update(path.file_name().unwrap_or_default().as_encoded_bytes());
        hasher.update(&bytes);
        games.extend(wthor::read(&bytes)?.1.into_iter().map(|game| game.moves));
    }
    Ok((games, check::hex(hasher.finalize().as_slice())))
}

/// Times each sampled node at each depth, one journal unit per pair, and
/// returns the times by depth.
pub fn pilot(
    sample: &[Key],
    nodes: &BTreeMap<Key, u32>,
    params: &Params,
    depths: &[u32],
    jobs: usize,
    path: &Path,
    header: &str,
) -> Result<BTreeMap<u32, Vec<f64>>, String> {
    let units: Vec<(String, (Key, u32))> = depths
        .iter()
        .flat_map(|&d| {
            sample
                .iter()
                .map(move |&k| (format!("{}:{d}", id(k)), (k, d)))
        })
        .collect();
    run_units(&units, params, jobs, path, header, |&(k, d), search| {
        let start = Instant::now();
        let drop = dropout(k, nodes, d, search);
        format!("{:.6} {drop:?}", start.elapsed().as_secs_f64())
    })?;
    let mut times: BTreeMap<u32, Vec<f64>> = BTreeMap::new();
    for record in journal::read(path)?.1 {
        let depth = record.id.rsplit(':').next().and_then(|d| d.parse().ok());
        let time = record.line.split(' ').next().and_then(|t| t.parse().ok());
        if let (Some(depth), Some(time)) = (depth, time) {
            times.entry(depth).or_default().push(time);
        }
    }
    Ok(times)
}

/// Writes move sequences one per line, in a8-style square names.
pub fn write_lines(lines: &[Vec<u8>], comment: &str) -> String {
    let name = |s: &u8| format!("{}{}", (b'a' + s % 8) as char, s / 8 + 1);
    let mut text = format!("# {comment}\n");
    for moves in lines {
        text.push_str(&moves.iter().map(name).collect::<Vec<_>>().join(" "));
        text.push('\n');
    }
    text
}

/// Reads lines written by `write_lines` into root keys with the side to move.
pub fn read_roots(text: &str) -> Result<Vec<Key>, String> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut board = Board::START;
            for name in line.split_whitespace() {
                let b = name.as_bytes();
                if b.len() != 2 || !(b'a'..=b'h').contains(&b[0]) || !(b'1'..=b'8').contains(&b[1])
                {
                    return Err(format!("invalid square {name}"));
                }
                let square = (b[1] - b'1') * 8 + b[0] - b'a';
                if legal_moves(board.me, board.opp) & (1 << square) == 0 {
                    return Err(format!("illegal move {name} in {line}"));
                }
                board = normalize(play(board, square)).0;
            }
            Ok(key(board))
        })
        .collect()
}
