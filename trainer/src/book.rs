//! Opening book builder (`docs/design/book.md`). Nodes are canonical,
//! normalized positions; values are centidiscs from the side to move.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use othello::board::{Board, final_score, is_terminal, legal_moves, pass, play, squares};
use othello::pattern;
use othello::rng::Rng;
use othello::score::{EVAL_BOUND, terminal_score};
use othello::search::{ProbCut, SearchConfig, SearchLimits, search_tt};
use othello::tt::{Table, zobrist};
use othello_tools::journal::{self, Journal};
use othello_tools::positions::{self, square_name};
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
/// The scorer's source commit, supplied by the build command.
const EXTENDER: &str = match option_env!("OTHELLO_COMMIT") {
    Some(commit) => commit,
    None => "unknown",
};
const RULES: &str = "normalize-passes,ascending-squares,clear-per-node,lower-square-ties";

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
/// A game with an illegal move is left out whole.
pub fn count(games: &[Vec<u8>], plies: u32) -> BTreeMap<Key, u32> {
    let mut counts = BTreeMap::new();
    for game in games {
        let Some(boards) = line(game) else { continue };
        let seen: BTreeSet<Key> = boards.into_iter().take(plies as usize).map(key).collect();
        for k in seen {
            *counts.entry(k).or_default() += 1;
        }
    }
    counts
}

/// The normalized board after each number of placements, or `None` if any
/// move is illegal. An unfinished game is valid.
fn line(moves: &[u8]) -> Option<Vec<Board>> {
    let mut boards = vec![Board::START];
    for &square in moves {
        let current = boards[boards.len() - 1];
        if square >= 64 || legal_moves(current.me, current.opp) & (1 << square) == 0 {
            return None;
        }
        boards.push(normalize(play(current, square)).0);
    }
    Some(boards)
}

pub fn legal_game(moves: &[u8]) -> bool {
    line(moves).is_some()
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
        let found = -value(play(node, m.square), depth.saturating_sub(1), search);
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
    truncation(roots, entries, &graph(entries), counts, mandatory, room).map(|(kept, _)| kept)
}

/// `truncate`, also returning the removals in order.
fn truncation(
    roots: &[(Key, bool)],
    entries: &BTreeMap<Key, Entry>,
    graph: &Graph,
    counts: &BTreeMap<Key, u32>,
    mandatory: &BTreeSet<Key>,
    room: usize,
) -> Result<(BTreeSet<Key>, Vec<Key>), String> {
    let mut roots = roots.to_vec();
    roots.extend(mandatory.iter().map(|&k| (k, true)));
    let (mut removed, mut order) = (BTreeSet::new(), Vec::new());
    loop {
        let records = walk(&roots, entries, graph, &removed);
        if let Some(k) = mandatory.iter().find(|k| !records.contains(k)) {
            return Err(format!("mandatory {} is not a movable book node", id(*k)));
        }
        if records.len() <= room {
            return Ok((records, order));
        }
        let victim = records
            .iter()
            .filter(|k| !mandatory.contains(k))
            .min_by_key(|&&k| (counts.get(&k).copied().unwrap_or(0), zobrist(board(k))))
            .ok_or_else(|| format!("{} mandatory records exceed room {room}", records.len()))?;
        removed.insert(*victim);
        order.push(*victim);
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
        let Some(boards) = line(game) else { continue };
        for (at, board) in boards
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

/// The collection inputs a journal must match to be built: version, N,
/// plies and the input description (data and node set).
pub fn identity(params: &Params, input: &str) -> String {
    format!(
        "book v1 min {} plies {} input {input}",
        params.min, params.plies
    )
}

pub fn header(params: &Params, input: &str) -> String {
    identity(params, input) + &tail(params)
}

/// The scoring fields after the identity, starting with a space.
fn tail(params: &Params) -> String {
    let config = policy();
    let eval = check::hex(&Sha256::digest(pattern::WEIGHTS));
    format!(
        " depth {} table {} eval {eval} engine {} \
         threshold {} ordering {} probcut {:?} rules {RULES}",
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
    if params.depth == 0 {
        return Err("depth must be at least 1".into());
    }
    let (journal, done) = Journal::open(path, header)?;
    let todo: Vec<_> = units.iter().filter(|(id, _)| !done.contains(id)).collect();
    let (skipped, journal, config) = (units.len() - todo.len(), Mutex::new(journal), policy());
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .map_err(|error| error.to_string())?;
    let (finished, stop, start) = (
        AtomicUsize::new(skipped),
        (Mutex::new(false), Condvar::new()),
        Instant::now(),
    );
    let every = std::env::var("OTHELLO_BOOK_PROGRESS_MS")
        .ok()
        .and_then(|ms| ms.parse().ok());
    let report = || {
        let (done, elapsed) = (
            finished.load(Ordering::Relaxed),
            start.elapsed().as_secs_f64(),
        );
        let eta = (units.len() - done) as f64 * elapsed / (done - skipped) as f64;
        println!(
            "progress units {done}/{} elapsed {elapsed:.0}s eta {eta:.0}s",
            units.len()
        );
    };
    report();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let wait = Duration::from_millis(every.unwrap_or(60_000));
            let running = |stopped: &mut bool| !*stopped;
            while !*stop
                .1
                .wait_timeout_while(stop.0.lock().unwrap(), wait, running)
                .unwrap()
                .0
            {
                report();
            }
        });
        let result: Result<(), String> = pool.install(|| {
            todo.par_iter().try_for_each_init(
                || Table::new(params.table),
                |table, (id, item)| {
                    table.clear();
                    let mut search = |b: Board, d: u32| {
                        search_tt(b, SearchLimits::depth(d), &config, table).score
                    };
                    let line = unit(item, &mut search);
                    journal.lock().unwrap().append(id, &line)?;
                    finished.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                },
            )
        });
        *stop.0.lock().unwrap() = true;
        stop.1.notify_all();
        result
    })?;
    Ok(skipped)
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
    score_in(nodes, &set, params, jobs, path, header)
}

/// `score`, partitioned against `context` instead of the units.
pub fn score_in(
    nodes: &[Key],
    set: &BTreeMap<Key, u32>,
    params: &Params,
    jobs: usize,
    path: &Path,
    header: &str,
) -> Result<(usize, usize), String> {
    let keys: BTreeSet<Key> = nodes.iter().copied().collect();
    let units: Vec<(String, Key)> = keys.iter().map(|&k| (id(k), k)).collect();
    let done = run_units(&units, params, jobs, path, header, |&k, search| {
        let mut scores = Vec::new();
        let mut recorded = |b: Board, d: u32| {
            let found = search(b, d);
            scores.push(found.to_string());
            found
        };
        let result = match dropout(k, set, params.depth, &mut recorded) {
            Some((value, square)) => format!("{value} {square}"),
            None => "none".into(),
        };
        format!("{result} | {}", scores.join(" "))
    })?;
    Ok((done, units.len() - done))
}

/// Whether a scoring header's tail after the identity is well formed: any
/// evaluator and engine, as long as each field has its type and range.
fn provenance(tail: &str) -> bool {
    let Some((fields, rules)) = tail.split_once(" rules ") else {
        return false;
    };
    let Some((fields, probcut)) = fields.split_once(" probcut ") else {
        return false;
    };
    let words: Vec<&str> = fields.split(' ').collect();
    let names = ["depth", "table", "eval", "engine", "threshold", "ordering"];
    let number = |at: usize, max: u64| {
        words[at]
            .parse::<u64>()
            .is_ok_and(|n| (1..=max).contains(&n))
    };
    words.len() == 13
        && words[0].is_empty()
        && (1..12).step_by(2).all(|at| words[at] == names[at / 2])
        && number(2, 60)
        && number(4, u32::MAX.into())
        && words[6].len() == 64
        && words[6].bytes().all(|b| b.is_ascii_hexdigit())
        && !words[8].is_empty()
        && number(10, 64)
        && ["true", "false"].contains(&words[12])
        && (probcut == "None" || probcut.starts_with("Some(ProbCut {") && probcut.ends_with("})"))
        && rules == RULES
}

/// The journal's drop-out results, refused unless the header matches
/// `identity`, every node has exactly one record, and each result is that
/// node's legal drop-out move (or none when it has none).
fn read_drops(
    path: &Path,
    nodes: &BTreeMap<Key, u32>,
    identity: &str,
) -> Result<(String, Drops), String> {
    let (header, records) = journal::read(path)?;
    if !header.strip_prefix(identity).is_some_and(provenance) {
        return Err(journal::mismatch(identity, &header));
    }
    let mut drops = BTreeMap::new();
    for record in records {
        let k = parse_id(&record.id)?;
        let outs: Vec<u8> = partition(k, nodes)
            .iter()
            .filter(|m| !m.book)
            .map(|m| m.square)
            .collect();
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
        let valid = drop.map_or(outs.is_empty(), |(value, square)| {
            outs.contains(&square) && (-EVAL_BOUND..=EVAL_BOUND).contains(&value)
        });
        if !nodes.contains_key(&k) || !valid || drops.insert(k, drop).is_some() {
            return Err(format!("foreign, invalid or repeated record {}", record.id));
        }
    }
    if drops.len() != nodes.len() {
        return Err(format!(
            "journal has {} of {} nodes",
            drops.len(),
            nodes.len()
        ));
    }
    Ok((header, drops))
}

/// Backs up the journal, prunes to the roots and the room, and returns the
/// asset and its manifest, both in asset order.
pub fn build(
    nodes: &BTreeMap<Key, u32>,
    path: &Path,
    identity: &str,
    roots: &[(Key, bool)],
    mandatory: &BTreeSet<Key>,
    room: usize,
) -> Result<(Vec<u8>, String), String> {
    let (header, drops) = read_drops(path, nodes, identity)?;
    let entries = backup(nodes, &drops)?;
    emit(
        nodes,
        &entries,
        &format!("# {header}\n"),
        roots,
        mandatory,
        room,
    )
}

/// Prunes `entries` to the roots and the room, and writes the asset and its
/// manifest after the `head` lines.
fn emit(
    nodes: &BTreeMap<Key, u32>,
    entries: &BTreeMap<Key, Entry>,
    head: &str,
    roots: &[(Key, bool)],
    mandatory: &BTreeSet<Key>,
    room: usize,
) -> Result<(Vec<u8>, String), String> {
    let graph = graph(entries);
    let (kept, removed) = truncation(roots, entries, &graph, nodes, mandatory, room / RECORD)?;
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
    let ids: Vec<String> = roots
        .iter()
        .map(|&(k, e)| format!("{}:{e}", id(k)))
        .collect();
    let mut manifest = format!(
        "{head}# nodes {} records {} room {room} roots {} {}\n",
        nodes.len(),
        rows.len(),
        roots.len(),
        check::hex(&Sha256::digest(ids.join(" ")))
    );
    for k in &removed {
        manifest.push_str(&format!("# removed {} count {}\n", id(*k), nodes[k]));
    }
    let gone: BTreeSet<Key> = removed.into_iter().collect();
    for &k in mandatory {
        let reach = |engine| {
            walk(&[(k, engine)], entries, &graph, &gone)
                .intersection(&kept)
                .count()
        };
        let replies = moves_where(k, |child| kept.contains(child));
        let stored = replies.iter().filter(|m| m.book && !m.passed).count();
        let line = format!(
            "# root {} first {} second {} replies {stored}/{}\n",
            id(k),
            reach(true),
            reach(false),
            replies.len()
        );
        manifest.push_str(&line);
    }
    for (_, k, square, value) in rows {
        manifest.push_str(&format!("{} {square} {}\n", id(k), clamp(value)));
    }
    Ok((asset, manifest))
}

/// The input description: the data hash and the node set's size and hash.
pub fn input(data: &str, nodes: &BTreeMap<Key, u32>) -> String {
    let ids: Vec<String> = nodes.keys().map(|&k| id(k)).collect();
    let hash = check::hex(&Sha256::digest(ids.join(" ")));
    format!("{data} nodes {}:{}", nodes.len(), &hash[..16])
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
    if depths.contains(&0) {
        return Err("depth must be at least 1".into());
    }
    run_units(&units, params, jobs, path, header, |&(k, d), search| {
        let start = Instant::now();
        let drop = dropout(k, nodes, d, search);
        format!("{:.6} {drop:?}", start.elapsed().as_secs_f64())
    })?;
    pilot_times(&units, path)
}

/// Saved pilot times by depth, in unit ID order, refusing foreign, repeated,
/// missing or invalid records.
fn pilot_times<U>(units: &[(String, U)], path: &Path) -> Result<BTreeMap<u32, Vec<f64>>, String> {
    let expected: BTreeSet<&str> = units.iter().map(|(id, _)| id.as_str()).collect();
    let mut saved = BTreeMap::new();
    for record in journal::read(path)?.1 {
        let time: f64 = record
            .line
            .split(' ')
            .next()
            .and_then(|t| t.parse().ok())
            .unwrap_or(-1.0);
        let fresh = expected.contains(record.id.as_str()) && time.is_finite() && time >= 0.0;
        if !fresh || saved.insert(record.id.clone(), time).is_some() {
            return Err(format!(
                "foreign, invalid or repeated pilot record {}",
                record.id
            ));
        }
    }
    if saved.len() != expected.len() {
        return Err(format!(
            "pilot has {} of {} units",
            saved.len(),
            expected.len()
        ));
    }
    let mut times: BTreeMap<u32, Vec<f64>> = BTreeMap::new();
    for (id, time) in saved {
        let depth = id
            .rsplit(':')
            .next()
            .and_then(|d| d.parse().ok())
            .ok_or("invalid pilot id")?;
        times.entry(depth).or_default().push(time);
    }
    Ok(times)
}

/// Move sequences one per line, after a comment line.
pub fn write_lines(lines: &[Vec<u8>], comment: &str) -> String {
    let named = lines.iter().map(|moves| {
        moves
            .iter()
            .map(|&s| square_name(s))
            .collect::<Vec<_>>()
            .join(" ")
    });
    std::iter::once(format!("# {comment}"))
        .chain(named)
        .map(|line| line + "\n")
        .collect()
}

/// Root keys of a file written by `write_lines`.
pub fn read_roots(text: &str) -> Result<Vec<Key>, String> {
    let boards = positions::parse(text)?
        .into_iter()
        .map(|opening| line(&opening.squares));
    boards
        .map(|boards| {
            boards
                .and_then(|b| b.last().map(|&board| key(board)))
                .ok_or("illegal root".into())
        })
        .collect()
}

/// The best of (square, value) pairs in square order: lower square on ties.
fn best(values: impl Iterator<Item = (u8, i32)>) -> Option<(i32, u8)> {
    values.fold(None, |best, (s, v)| {
        if best.is_none_or(|(b, _)| v > b) {
            Some((v, s))
        } else {
            best
        }
    })
}

/// Parent values of a record's drop-out squares under `context`: a terminal
/// child takes no token, a child that must pass +t, any other −t. The
/// tokens must be consumed exactly, in the score domain, and agree with the
/// saved best result.
pub fn decode(
    key: Key,
    context: &BTreeMap<Key, u32>,
    line: &str,
) -> Result<Vec<(u8, i32)>, String> {
    let invalid = || format!("invalid record {}", id(key));
    let (saved, tokens) = line.split_once(" | ").ok_or_else(invalid)?;
    let mut tokens = tokens.split(' ').filter(|t| !t.is_empty()).map(|t| {
        t.parse()
            .ok()
            .filter(|v| (-EVAL_BOUND..=EVAL_BOUND).contains(v))
    });
    let node = board(key);
    let mut values = Vec::new();
    for m in partition(key, context).into_iter().filter(|m| !m.book) {
        let child = play(node, m.square);
        let value = if is_terminal(child) {
            -terminal_score(final_score(child))
        } else {
            let token = tokens.next().flatten().ok_or_else(invalid)?;
            if legal_moves(child.me, child.opp) == 0 {
                token
            } else {
                -token
            }
        };
        values.push((m.square, value));
    }
    let parsed = match saved.split_once(' ') {
        None if saved == "none" => None,
        Some((v, s)) => Some((
            v.parse().map_err(|_| invalid())?,
            s.parse().map_err(|_| invalid())?,
        )),
        None => return Err(invalid()),
    };
    if tokens.next().is_some() || parsed != best(values.iter().copied()) {
        return Err(invalid());
    }
    Ok(values)
}

/// `x(p)` over the decoded squares that are still drop-outs in `nodes`.
pub fn remaining(key: Key, values: &[(u8, i32)], nodes: &BTreeMap<Key, u32>) -> Option<(i32, u8)> {
    let node = board(key);
    best(
        values
            .iter()
            .copied()
            .filter(|&(s, _)| !nodes.contains_key(&self::key(normalize(play(node, s)).0))),
    )
}

/// Untruncated walk records whose backed-up move is a drop-out.
pub fn departures(roots: &[(Key, bool)], entries: &BTreeMap<Key, Entry>) -> Vec<Key> {
    walk(roots, entries, &graph(entries), &BTreeSet::new())
        .into_iter()
        .filter(|k| !entries[k].book)
        .collect()
}

/// The children the keys' backed-up moves lead to that are neither nodes
/// nor terminal, sorted and distinct.
pub fn phase_a(keys: &[Key], entries: &BTreeMap<Key, Entry>) -> Vec<Key> {
    let children: BTreeSet<Key> = keys
        .iter()
        .filter_map(|k| Some((board(*k), entries.get(k)?.square?)))
        .map(|(node, s)| key(normalize(play(node, s)).0))
        .filter(|k| !entries.contains_key(k) && !is_terminal(board(*k)))
        .collect();
    children.into_iter().collect()
}

/// Phase B units: the same rule over the A units, after the post-A backup.
pub fn phase_b(units: &[Key], entries: &BTreeMap<Key, Entry>) -> Vec<Key> {
    phase_a(units, entries)
}

/// Extension rounds: the phase journal directory and the rounds requested.
pub struct Extension<'a> {
    pub dir: &'a Path,
    pub rounds: u32,
}

/// Nodes with each node's decoded drop-out values.
#[derive(Clone)]
struct State {
    nodes: BTreeMap<Key, u32>,
    values: BTreeMap<Key, Vec<(u8, i32)>>,
}

impl State {
    fn entries(&self) -> Result<BTreeMap<Key, Entry>, String> {
        let drops: Drops = self
            .values
            .iter()
            .map(|(&k, values)| (k, remaining(k, values, &self.nodes)))
            .collect();
        backup(&self.nodes, &drops)
    }

    /// Decodes `path` under `context`: each record a unit, at most once.
    /// Returns its sorted logical lines and whether every unit is present.
    fn add(
        &mut self,
        path: &Path,
        units: &[Key],
        context: &BTreeMap<Key, u32>,
    ) -> Result<(Vec<String>, bool), String> {
        let (_, records) = journal::read(path)?;
        let mut lines = Vec::new();
        for record in records {
            let k = parse_id(&record.id)?;
            let values = decode(k, context, &record.line)?;
            if units.binary_search(&k).is_err() || self.values.insert(k, values).is_some() {
                return Err(format!(
                    "foreign or repeated unit {} in {}",
                    record.id,
                    path.display()
                ));
            }
            lines.push(format!("{}\t{}", record.id, record.line));
        }
        lines.sort();
        let complete = lines.len() == units.len();
        if complete {
            self.nodes = context.clone();
        }
        Ok((lines, complete))
    }
}

/// `{count}:{sha256}` of `text`.
fn digest(count: usize, text: &str) -> String {
    format!("{count}:{}", check::hex(&Sha256::digest(text)))
}

/// One round's counts, and the book after it.
struct Round {
    departures: Vec<Key>,
    a: Vec<Key>,
    b: usize,
    peers: usize,
    into: usize,
    entries: BTreeMap<Key, Entry>,
}

/// The base journal decoded under the node set, its header checked against
/// the identity and, with `params`, against this binary's scoring policy
/// and evaluator (the engine commit may differ).
fn load(
    nodes: &BTreeMap<Key, u32>,
    path: &Path,
    identity: &str,
    params: Option<&Params>,
) -> Result<(String, State, String), String> {
    let header = journal::read(path)?.0;
    let tail = header
        .strip_prefix(identity)
        .filter(|tail| provenance(tail));
    let mask = |tail: &str| -> Vec<String> {
        let mut words: Vec<String> = tail.split(' ').map(String::from).collect();
        words[8].clear();
        words
    };
    let policy = |tail: &str| params.is_none_or(|p| mask(tail) == mask(&self::tail(p)));
    if !tail.is_some_and(policy) {
        return Err(journal::mismatch(identity, &header));
    }
    let mut state = State {
        nodes: nodes.clone(),
        values: BTreeMap::new(),
    };
    let units: Vec<Key> = nodes.keys().copied().collect();
    let (lines, complete) = state.add(path, &units, nodes)?;
    if !complete {
        return Err(format!(
            "base journal has {} of {} nodes",
            lines.len(),
            nodes.len()
        ));
    }
    Ok((header, state, lines.join("\n")))
}

/// Replays (and with `score`, runs) the extension phases in order, each
/// header binding its inputs. Returns the state after the last complete
/// round, the rounds, and a note on a partial phase after it.
fn rounds(
    nodes: &BTreeMap<Key, u32>,
    base: &Path,
    identity: &str,
    roots: &[(Key, bool)],
    extension: &Extension,
    score: Option<(&Params, usize)>,
) -> Result<(State, Vec<Round>, String), String> {
    let (base_header, mut state, base_text) = load(nodes, base, identity, score.map(|s| s.0))?;
    let roots_text: Vec<String> = roots
        .iter()
        .map(|&(k, e)| format!("{}:{e}", id(k)))
        .collect();
    let (mut prior, mut prior_count, mut done) = (String::new(), 0, Vec::new());
    // A run declares R and its own scorer; a build replays what the run
    // recorded, so the builder's identity never enters.
    let recorded = journal::read(&extension.dir.join("r1a.journal")).ok();
    let field = |name: &str| -> Option<String> {
        let (header, _) = recorded.as_ref()?;
        Some(
            header
                .split(&format!(" {name} "))
                .nth(1)?
                .split(' ')
                .next()?
                .into(),
        )
    };
    let (total, extender) = match score {
        Some(_) => (extension.rounds, EXTENDER.to_string()),
        None => (
            field("extend R")
                .and_then(|r| r.parse().ok())
                .unwrap_or(extension.rounds),
            field("extender").unwrap_or(EXTENDER.into()),
        ),
    };
    let mut finished = state.clone();
    for r in 1..=extension.rounds {
        let before = state.entries()?;
        let departures = departures(roots, &before);
        let mut a = Vec::new();
        for phase in ["a", "b"] {
            let units = if phase == "a" {
                phase_a(&departures, &before)
            } else {
                phase_b(&a, &state.entries()?)
            };
            let mut context = state.nodes.clone();
            context.extend(units.iter().map(|&k| (k, 0)));
            let ids: Vec<String> = units.iter().map(|&k| id(k)).collect();
            let all: Vec<String> = context.keys().map(|&k| id(k)).collect();
            let header = format!(
                "{base_header} extend R {total} round {r} phase {phase} base {} prior {} roots {} units {} context {} extender {extender}",
                digest(nodes.len(), &base_text),
                digest(prior_count, &prior),
                digest(roots.len(), &roots_text.join(" ")),
                digest(units.len(), &ids.join("\n")),
                digest(context.len(), &all.join("\n")),
            );
            let path = extension.dir.join(format!("r{r}{phase}.journal"));
            if path.exists() {
                let found = journal::read(&path)?.0;
                if found != header {
                    return Err(journal::mismatch(&header, &found));
                }
                state.clone().add(&path, &units, &context)?;
            }
            if let Some((params, jobs)) = score {
                std::fs::create_dir_all(extension.dir).map_err(|error| error.to_string())?;
                score_in(&units, &context, params, jobs, &path, &header)?;
            } else if !path.exists() {
                return Ok((finished, done, String::new()));
            }
            let (lines, complete) = state.add(&path, &units, &context)?;
            if !complete {
                let note = format!(
                    "# partial round {r} phase {phase} {}/{}\n",
                    lines.len(),
                    units.len()
                );
                return Ok((finished, done, note));
            }
            prior.push_str(&format!("r{r}{phase}\n{}\n", lines.join("\n")));
            prior_count += lines.len();
            if phase == "a" {
                a = units;
            } else {
                let peers = |into: &dyn Fn(&Key) -> bool| -> Vec<usize> {
                    a.iter()
                        .map(|&k| {
                            moves_where(k, |c| into(c))
                                .iter()
                                .filter(|m| m.book)
                                .count()
                        })
                        .collect()
                };
                let in_a = |c: &Key| a.binary_search(c).is_ok();
                done.push(Round {
                    peers: peers(&in_a).iter().sum(),
                    into: peers(&|c: &Key| before.contains_key(c))
                        .iter()
                        .filter(|&&n| n > 0)
                        .count(),
                    departures: departures.clone(),
                    a: a.clone(),
                    b: units.len(),
                    entries: state.entries()?,
                });
                finished = state.clone();
            }
        }
    }
    Ok((finished, done, String::new()))
}

/// Runs or resumes the extension rounds over the base journal, returning
/// one log line per round.
pub fn extend(
    nodes: &BTreeMap<Key, u32>,
    base: &Path,
    identity: &str,
    roots: &[(Key, bool)],
    extension: &Extension,
    params: &Params,
    jobs: usize,
) -> Result<String, String> {
    let (_, done, _) = rounds(
        nodes,
        base,
        identity,
        roots,
        extension,
        Some((params, jobs)),
    )?;
    Ok(done
        .iter()
        .enumerate()
        .map(|(r, round)| {
            format!(
                "round {} departures {} a {} b {}\n",
                r + 1,
                round.departures.len(),
                round.a.len(),
                round.b
            )
        })
        .collect())
}

/// `build` over the base journal and the complete extension rounds; with
/// no rounds, exactly `build`.
pub fn build_extended(
    nodes: &BTreeMap<Key, u32>,
    base: &Path,
    identity: &str,
    roots: &[(Key, bool)],
    mandatory: &BTreeSet<Key>,
    room: usize,
    extension: &Extension,
) -> Result<(Vec<u8>, String), String> {
    if extension.rounds == 0 {
        return build(nodes, base, identity, roots, mandatory, room);
    }
    let (state, done, note) = rounds(nodes, base, identity, roots, extension, None)?;
    let header = journal::read(base)?.0;
    let mut head = format!(
        "# {header}\n# rounds requested {} completed {}\n{note}",
        extension.rounds,
        done.len()
    );
    for (r, round) in done.iter().enumerate() {
        let reach = |engine| -> usize {
            let graph = graph(&round.entries);
            mandatory
                .iter()
                .map(|&k| walk(&[(k, engine)], &round.entries, &graph, &BTreeSet::new()).len())
                .sum()
        };
        let returned = round
            .departures
            .iter()
            .filter(|k| round.entries[k].book)
            .count();
        head.push_str(&format!(
            "# round {} departures {} a {} b {} a-peers {} into {} returned {returned} records {} first {} second {}\n",
            r + 1,
            round.departures.len(),
            round.a.len(),
            round.b,
            round.peers,
            round.into,
            walk(roots, &round.entries, &graph(&round.entries), &BTreeSet::new()).len(),
            reach(true),
            reach(false),
        ));
    }
    emit(
        &state.nodes,
        &state.entries()?,
        &head,
        roots,
        mandatory,
        room,
    )
}
