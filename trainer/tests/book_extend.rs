//! Extension rounds (design amendment 3).

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use othello::board::{Board, final_score, is_terminal, legal_moves, pass, play, squares};
use othello::rng::Rng;
use othello::score::{EVAL_BOUND, terminal_score};
use othello::search::{SearchLimits, search_tt};
use othello_tools::journal::{self, Journal};
use othello_trainer::book::{self, Entry, Extension, Key, Params};
use sha2::{Digest, Sha256};

const START: Key = (Board::START.me, Board::START.opp);

const PARAMS: Params = Params {
    min: 2,
    plies: 12,
    depth: 2,
    table: 1 << 12,
};

fn node_set(keys: impl IntoIterator<Item = Key>) -> BTreeMap<Key, u32> {
    keys.into_iter().map(|key| (key, 10)).collect()
}

fn child_key(board: Board, square: u8) -> Key {
    book::key(book::normalize(play(board, square)).0)
}

fn legal(key: Key) -> Vec<u8> {
    let b = book::board(key);
    squares(legal_moves(b.me, b.opp)).collect()
}

/// The first square to each distinct canonical child, in square order.
fn distinct(key: Key) -> Vec<u8> {
    let mut seen = BTreeSet::new();
    legal(key)
        .into_iter()
        .filter(|&s| seen.insert(child_key(book::board(key), s)))
        .collect()
}

fn needs_pass(board: Board) -> bool {
    legal_moves(board.me, board.opp) == 0 && !is_terminal(board)
}

fn games(count: usize, families: u64, shared: usize, plies: usize) -> Vec<Vec<u8>> {
    (0..count as u64)
        .map(|game| {
            let (mut board, mut moves) = (Board::START, Vec::new());
            let (mut family, mut own) = (Rng::new(1 + game % families), Rng::new(100 + game));
            while moves.len() < plies && !is_terminal(board) {
                if legal_moves(board.me, board.opp) == 0 {
                    board = pass(board);
                }
                let legal: Vec<u8> = squares(legal_moves(board.me, board.opp)).collect();
                let rng = if moves.len() < shared {
                    &mut family
                } else {
                    &mut own
                };
                let square = legal[(rng.next_u64() % legal.len() as u64) as usize];
                moves.push(square);
                board = play(board, square);
            }
            moves
        })
        .collect()
}

/// The first canonical (node, square) in random playouts where `want` holds.
fn find(want: impl Fn(Board, u8) -> bool) -> (Key, u8) {
    for seed in 1..20_000 {
        let mut rng = Rng::new(seed);
        let mut board = Board::START;
        while !is_terminal(board) {
            if legal_moves(board.me, board.opp) == 0 {
                board = pass(board);
            }
            let k = book::key(board);
            let node = book::board(k);
            for square in squares(legal_moves(node.me, node.opp)) {
                if want(node, square) {
                    return (k, square);
                }
            }
            let legal: Vec<u8> = squares(legal_moves(board.me, board.opp)).collect();
            board = play(board, legal[(rng.next_u64() % legal.len() as u64) as usize]);
        }
    }
    panic!("no fixture found");
}

/// Canonical parents of every position up to `plies` placements.
fn parents(plies: usize) -> BTreeMap<Key, Vec<(Key, u8)>> {
    let mut parents: BTreeMap<Key, Vec<(Key, u8)>> = BTreeMap::new();
    let mut level = BTreeSet::from([START]);
    for _ in 0..plies {
        let mut next = BTreeSet::new();
        for &k in &level {
            for s in legal(k) {
                let child = child_key(book::board(k), s);
                if !is_terminal(book::board(child)) {
                    parents.entry(child).or_default().push((k, s));
                    next.insert(child);
                }
            }
        }
        level = next;
    }
    parents
}

/// Two distinct parents (p1, s1) and (p2, s2) of one child.
fn transposition() -> ((Key, u8), (Key, u8), Key) {
    for (child, list) in parents(6) {
        if let Some(&other) = list.iter().find(|p| p.0 != list[0].0) {
            return (list[0], other, child);
        }
    }
    panic!("no transposition");
}

/// (p1, s1) reaching c1 and (p2, s2) reaching c2, where c2 is also a child of
/// c1: a same-phase peer edge once c1 and c2 are both A units.
fn peer() -> ((Key, u8), Key, (Key, u8), Key) {
    let parents = parents(5);
    for (&c2, list) in &parents {
        for &(c1, _) in list {
            let Some(&(p2, s2)) = list.iter().find(|p| p.0 != c1) else {
                continue;
            };
            let Some(&(p1, s1)) = parents
                .get(&c1)
                .and_then(|l| l.iter().find(|p| p.0 != p2 && p.0 != c2))
            else {
                continue;
            };
            if p1 != c2 && p2 != c1 {
                return ((p1, s1), c1, (p2, s2), c2);
            }
        }
    }
    panic!("no peer edge");
}

fn entry(value: i32, square: u8, book: bool) -> Entry {
    Entry {
        value,
        square: Some(square),
        book,
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("othello-extend-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sorted_lines(path: &Path) -> Vec<String> {
    let (_, records) = journal::read(path).unwrap();
    let mut lines: Vec<String> = records
        .iter()
        .map(|record| format!("{}\t{}", record.id, record.line))
        .collect();
    lines.sort();
    lines
}

fn id(k: Key) -> String {
    format!("{:016x}:{:016x}", k.0, k.1)
}

// ---- Decoder ------------------------------------------------------------

/// The expected decoding of `tokens` for `key` under `context`: one token
/// per non-terminal drop-out, +t after a forced pass, −t otherwise.
fn expected_values(key: Key, context: &BTreeMap<Key, u32>, tokens: &[i32]) -> Vec<(u8, i32)> {
    let node = book::board(key);
    let mut tokens = tokens.iter();
    legal(key)
        .into_iter()
        .filter(|&s| !context.contains_key(&child_key(node, s)))
        .map(|s| {
            let b = play(node, s);
            let value = if is_terminal(b) {
                -terminal_score(final_score(b))
            } else if needs_pass(b) {
                *tokens.next().unwrap()
            } else {
                -*tokens.next().unwrap()
            };
            (s, value)
        })
        .collect()
}

fn token_count(key: Key, context: &BTreeMap<Key, u32>) -> usize {
    let node = book::board(key);
    legal(key)
        .into_iter()
        .filter(|&s| !context.contains_key(&child_key(node, s)))
        .filter(|&s| !is_terminal(play(node, s)))
        .count()
}

fn best_of(values: &[(u8, i32)]) -> Option<(i32, u8)> {
    values
        .iter()
        .map(|&(s, v)| (v, s))
        .max_by_key(|&(v, s)| (v, std::cmp::Reverse(s)))
}

fn line_of(best: Option<(i32, u8)>, tokens: &[i32]) -> String {
    let best = best.map_or("none".into(), |(v, s)| format!("{v} {s}"));
    let tokens: Vec<String> = tokens.iter().map(i32::to_string).collect();
    format!("{best} | {}", tokens.join(" "))
}

/// Decodes a node with an interspersed special child (terminal or forced
/// pass), then refuses each malformed variant of the same record.
fn decode_fixture(kind: impl Fn(Board) -> bool) {
    let (key, _) = find(|board, square| {
        kind(play(board, square)) && squares(legal_moves(board.me, board.opp)).count() >= 3
    });
    let context = node_set([key]);
    let n = token_count(key, &context);
    let tokens: Vec<i32> = (0..n as i32).map(|i| 37 * i - 50).collect();
    let values = expected_values(key, &context, &tokens);
    assert!(legal(key).iter().any(|&s| kind(play(book::board(key), s))));
    let line = line_of(best_of(&values), &tokens);
    assert_eq!(book::decode(key, &context, &line).unwrap(), values);

    let bad = |line: String| assert!(book::decode(key, &context, &line).is_err(), "{line}");
    let best = best_of(&values);
    bad(line_of(best, &tokens[..n - 1]));
    bad(line_of(best, &[tokens.clone(), vec![1]].concat()));
    let rest: Vec<String> = tokens[1..].iter().map(i32::to_string).collect();
    let (v, s) = best.unwrap();
    bad(format!("{v} {s} | x {}", rest.join(" ")));
    let mut wide = tokens.clone();
    wide[0] = EVAL_BOUND + 1;
    bad(line_of(best, &wide));
    bad(line_of(Some((v + 1, s)), &tokens));
    let other = legal(key).into_iter().find(|&q| q != s).unwrap();
    bad(line_of(Some((v, other)), &tokens));
    bad(line_of(None, &tokens));
}

#[test]
fn decode_terminal_children() {
    decode_fixture(is_terminal);
}

#[test]
fn decode_pass_children() {
    decode_fixture(|child| needs_pass(child) && !is_terminal(pass(child)));
}

#[test]
fn decode_uses_context() {
    // A square whose child is in the context is a book move: no token.
    let one = book::key(play(Board::START, 19));
    let squares_ = legal(one);
    let book_child = child_key(book::board(one), squares_[0]);
    let context = node_set([one, book_child]);
    let n = token_count(one, &context);
    assert!(n < squares_.len());
    let tokens: Vec<i32> = (0..n as i32).map(|i| 11 * i).collect();
    let values = expected_values(one, &context, &tokens);
    assert!(values.iter().all(|&(s, _)| s != squares_[0]));
    let line = line_of(best_of(&values), &tokens);
    assert_eq!(book::decode(one, &context, &line).unwrap(), values);
    // The same record is refused under a context it was not scored in.
    assert!(book::decode(one, &node_set([one]), &line).is_err());
}

/// Independent oracle: a child's value under the policy, fresh table.
fn policy_value(board: Board, depth: u32) -> i32 {
    if is_terminal(board) {
        terminal_score(final_score(board))
    } else if legal_moves(board.me, board.opp) == 0 {
        -policy_value(pass(board), depth)
    } else {
        let mut table = othello::tt::Table::new(1 << 12);
        search_tt(
            board,
            SearchLimits::depth(depth),
            &book::policy(),
            &mut table,
        )
        .score
    }
}

#[test]
fn decode_matches_search() {
    // Records written by the scorer decode to the searched parent values.
    let list = games(30, 3, 10, 40);
    let nodes: BTreeMap<Key, u32> = book::count(&list, 12)
        .into_iter()
        .filter(|&(_, n)| n >= 2)
        .collect();
    let keys: Vec<Key> = nodes.keys().copied().collect();
    let dir = temp_dir("decode");
    let path = dir.join("base.journal");
    book::score(&keys, &PARAMS, 2, &path, &book::header(&PARAMS, "test")).unwrap();
    let (_, records) = journal::read(&path).unwrap();
    assert_eq!(records.len(), keys.len());
    let mut checked = 0;
    for record in records {
        let k = keys.iter().copied().find(|&k| id(k) == record.id).unwrap();
        let node = book::board(k);
        let values = book::decode(k, &nodes, &record.line).unwrap();
        let expected: Vec<(u8, i32)> = legal(k)
            .into_iter()
            .filter(|&s| !nodes.contains_key(&child_key(node, s)))
            .map(|s| (s, -policy_value(play(node, s), PARAMS.depth - 1)))
            .collect();
        checked += expected.len();
        assert_eq!(values, expected);
    }
    assert!(checked > 0);
    std::fs::remove_dir_all(dir).unwrap();
}

// ---- Reclassification ----------------------------------------------------

#[test]
fn remaining_promotes_squares() {
    let one = book::key(play(Board::START, 19));
    let node = book::board(one);
    let s = distinct(one);
    assert!(s.len() >= 3);
    let values: Vec<(u8, i32)> = vec![(s[0], 500), (s[1], 300), (s[2], 300)];
    // Nothing promoted: the best decoded value.
    assert_eq!(
        book::remaining(one, &values, &node_set([one])),
        Some((500, s[0]))
    );
    // The old best promoted: the runner-up, lower square on the tie.
    let promoted = node_set([one, child_key(node, s[0])]);
    assert_eq!(book::remaining(one, &values, &promoted), Some((300, s[1])));
    // Every decoded square promoted: absent.
    let all = node_set(
        [one]
            .into_iter()
            .chain(s[..3].iter().map(|&q| child_key(node, q))),
    );
    assert_eq!(book::remaining(one, &values, &all), None);
}

#[test]
fn remaining_transposed_squares() {
    // All four start moves reach one canonical child: adding it promotes
    // every square at once.
    let values: Vec<(u8, i32)> = legal(START).into_iter().map(|s| (s, s as i32)).collect();
    assert_eq!(values.len(), 4);
    let child = child_key(Board::START, 19);
    assert_eq!(
        book::remaining(START, &values, &node_set([START, child])),
        None
    );
}

#[test]
fn backup_through_child() {
    // The old best drop-out square becomes a book move; backup then takes
    // −v(child) when it beats the runner-up, and the runner-up otherwise.
    let one = book::key(play(Board::START, 19));
    let node = book::board(one);
    let s = distinct(one);
    let child = child_key(node, s[0]);
    let values: Vec<(u8, i32)> = vec![(s[0], 900), (s[1], 200)];
    let nodes = node_set([one, child]);
    let child_square = legal(child)[0];
    for (child_value, expected) in [
        (-400, entry(400, s[0], true)),
        (100, entry(200, s[1], false)),
    ] {
        let mut drops = book::Drops::new();
        drops.insert(one, book::remaining(one, &values, &nodes));
        drops.insert(child, Some((child_value, child_square)));
        let entries = book::backup(&nodes, &drops).unwrap();
        assert_eq!(entries[&one], expected);
    }
}

// ---- Departures and phases ------------------------------------------------

/// The start, `one`, and every child of `one`, each child choosing a
/// drop-out; plus an unreachable node that also chooses a drop-out.
fn departure_book() -> (BTreeMap<Key, u32>, BTreeMap<Key, Entry>, Vec<Key>, Key) {
    let one = book::key(play(Board::START, 19));
    let twos: BTreeSet<Key> = legal(one)
        .into_iter()
        .map(|s| child_key(book::board(one), s))
        .collect();
    let (stray, _) = find(|board, _| (board.me | board.opp).count_ones() > 20);
    let mut nodes = node_set([START, one, stray]);
    nodes.extend(node_set(twos.iter().copied()));
    let mut drops = book::Drops::new();
    drops.insert(START, None);
    drops.insert(one, None);
    drops.insert(stray, Some((0, legal(stray)[0])));
    for (i, &two) in twos.iter().enumerate() {
        drops.insert(two, Some((10 * i as i32, legal(two)[0])));
    }
    let entries = book::backup(&nodes, &drops).unwrap();
    (nodes, entries, twos.into_iter().collect(), stray)
}

#[test]
fn departures_from_walk() {
    let (_, entries, twos, stray) = departure_book();
    // Engine first: every reply position is a record choosing a drop-out.
    assert_eq!(book::departures(&[(START, true)], &entries), twos);
    // Engine second: its record at `one` chooses a book move.
    assert!(book::departures(&[(START, false)], &entries).is_empty());
    // Roots are walked as given; the stray joins only as a root.
    let both = book::departures(&[(START, true), (stray, true)], &entries);
    let mut expected = twos.clone();
    expected.push(stray);
    expected.sort();
    assert_eq!(both, expected);
    assert!(!book::departures(&[(START, true)], &entries).contains(&stray));
}

#[test]
fn departures_after_pass() {
    // A passing book move keeps the engine to move; the child it reaches is
    // then a record, and a departure when it chooses a drop-out.
    let (parent, square) = find(|board, square| {
        let child = play(board, square);
        needs_pass(child) && !is_terminal(pass(child))
    });
    let child = child_key(book::board(parent), square);
    let nodes = node_set([parent, child]);
    let mut drops = book::Drops::new();
    drops.insert(parent, None);
    drops.insert(child, Some((50, legal(child)[0])));
    let entries = book::backup(&nodes, &drops).unwrap();
    assert_eq!(entries[&parent].square, Some(square));
    assert_eq!(book::departures(&[(parent, true)], &entries), vec![child]);
    assert!(book::departures(&[(parent, false)], &entries).is_empty());
}

#[test]
fn phase_a_units() {
    let (_, entries, twos, _) = departure_book();
    let mut expected: Vec<Key> = twos
        .iter()
        .map(|&two| child_key(book::board(two), entries[&two].square.unwrap()))
        .filter(|k| !entries.contains_key(k) && !is_terminal(book::board(*k)))
        .collect();
    expected.sort();
    expected.dedup();
    assert!(!expected.is_empty());
    assert_eq!(book::phase_a(&twos, &entries), expected);
}

#[test]
fn phase_a_transpositions() {
    // Two departures reaching one child give one unit.
    let ((p1, s1), (p2, s2), child) = transposition();
    let nodes = node_set([p1, p2]);
    let mut drops = book::Drops::new();
    drops.insert(p1, Some((100, s1)));
    drops.insert(p2, Some((100, s2)));
    let entries = book::backup(&nodes, &drops).unwrap();
    let mut departures = vec![p1, p2];
    departures.sort();
    assert_eq!(book::phase_a(&departures, &entries), vec![child]);
    // A departure into a terminal board gives none.
    let (end, square) = find(|board, square| is_terminal(play(board, square)));
    let nodes = node_set([end]);
    let mut drops = book::Drops::new();
    drops.insert(end, Some((100, square)));
    let entries = book::backup(&nodes, &drops).unwrap();
    assert!(book::phase_a(&[end], &entries).is_empty());
}

#[test]
fn phase_a_passes() {
    // The unit is the normalized (passed) child.
    let (parent, square) = find(|board, square| {
        let child = play(board, square);
        needs_pass(child) && !is_terminal(pass(child))
    });
    let nodes = node_set([parent]);
    let mut drops = book::Drops::new();
    drops.insert(parent, Some((100, square)));
    let entries = book::backup(&nodes, &drops).unwrap();
    let unit = book::phase_a(&[parent], &entries);
    let passed = book::key(pass(play(book::board(parent), square)));
    assert_eq!(unit, vec![passed]);
}

#[test]
fn phase_b_selection() {
    let one = book::key(play(Board::START, 19));
    let node = book::board(one);
    let s = distinct(one);
    let (low, high) = (s[0], s[s.len() - 1]);
    let high_child = child_key(node, high);
    let low_child = child_key(node, low);
    assert_ne!(low_child, high_child);
    let nodes = node_set([one, high_child]);
    let high_square = legal(high_child)[0];
    let run = |drop: Option<(i32, u8)>, child: i32| {
        let mut drops = book::Drops::new();
        drops.insert(one, drop);
        drops.insert(high_child, Some((child, high_square)));
        book::phase_b(&[one], &book::backup(&nodes, &drops).unwrap())
    };
    // A book move beating the best drop-out adds no unit.
    assert!(run(Some((100, low)), -300).is_empty());
    // A better drop-out adds its child.
    assert_eq!(run(Some((300, low)), -100), vec![low_child]);
    // Equal values: the lower square, here the drop-out, wins.
    assert_eq!(run(Some((200, low)), -200), vec![low_child]);
    // Every move a book move: no drop-out, no unit.
    let all = node_set(
        [one]
            .into_iter()
            .chain(s.iter().map(|&q| child_key(node, q))),
    );
    let mut drops = book::Drops::new();
    drops.insert(one, None);
    for &k in all.keys().filter(|&&k| k != one) {
        drops.insert(k, Some((0, legal(k)[0])));
    }
    assert!(book::phase_b(&[one], &book::backup(&all, &drops).unwrap()).is_empty());
}

#[test]
fn phase_b_passes() {
    // A selected move needing a pass gives the normalized child.
    let (c, square) = find(|board, square| {
        let child = play(board, square);
        needs_pass(child) && !is_terminal(pass(child))
    });
    let mut entries = BTreeMap::from([(c, entry(100, square, false))]);
    let passed = book::key(pass(play(book::board(c), square)));
    assert_eq!(book::phase_b(&[c], &entries), vec![passed]);
    // A selected move into a terminal board gives none.
    let (end, square) = find(|board, square| is_terminal(play(board, square)));
    entries.insert(end, entry(100, square, false));
    assert_eq!(book::phase_b(&[end], &entries), Vec::<Key>::new());
    // Units are sorted and distinct whatever the input order.
    let mut both = vec![c, end];
    both.reverse();
    assert_eq!(book::phase_b(&both, &entries), vec![passed]);
}

// ---- Phase scoring --------------------------------------------------------

#[test]
fn score_in_peers() {
    // c1 and c2 are both units, and c2 is a child of c1: under the phase
    // context that move is a book move, so c1's record has no token for it.
    let ((p1, s1), c1, (p2, s2), c2) = peer();
    let nodes = node_set([p1, p2]);
    let mut drops = book::Drops::new();
    drops.insert(p1, Some((100, s1)));
    drops.insert(p2, Some((100, s2)));
    let entries = book::backup(&nodes, &drops).unwrap();
    let mut departures = vec![p1, p2];
    departures.sort();
    let mut units = vec![c1, c2];
    units.sort();
    assert_eq!(book::phase_a(&departures, &entries), units);
    let mut context = nodes.clone();
    context.extend(units.iter().map(|&k| (k, 0)));
    let dir = temp_dir("peer");
    let header = "peer test";
    let run = |name: &str, order: &[Key], jobs: usize| {
        let path = dir.join(name);
        book::score_in(order, &context, &PARAMS, jobs, &path, header).unwrap();
        sorted_lines(&path)
    };
    let mut reversed = units.clone();
    reversed.reverse();
    let one = run("one", &units, 1);
    assert_eq!(one, run("reversed", &reversed, 3));
    let line = one.iter().find(|l| l.starts_with(&id(c1))).unwrap();
    let record = line.split_once('\t').unwrap().1;
    let values = book::decode(c1, &context, record).unwrap();
    let to_c2: Vec<u8> = legal(c1)
        .into_iter()
        .filter(|&s| child_key(book::board(c1), s) == c2)
        .collect();
    assert!(!to_c2.is_empty());
    assert!(values.iter().all(|(s, _)| !to_c2.contains(s)));
    // Without c2 in the context the record no longer decodes.
    let mut without = context.clone();
    without.remove(&c2);
    assert!(book::decode(c1, &without, record).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- Rounds, identity and build ------------------------------------------

/// A scored base over a small game set, with its node set and identity.
struct Base {
    dir: PathBuf,
    nodes: BTreeMap<Key, u32>,
    journal: PathBuf,
}

fn base(name: &str, depth: u32) -> Base {
    let dir = temp_dir(name);
    let games = games(30, 3, 10, 40);
    std::fs::write(dir.join("games.wtb"), archive(&games)).unwrap();
    let nodes: BTreeMap<Key, u32> = book::count(&games, 12)
        .into_iter()
        .filter(|&(_, n)| n >= 2)
        .collect();
    let journal = dir.join("base.journal");
    let output = trainer(&base_args(&dir, &journal, depth));
    assert!(output.status.success(), "{}", stderr(&output));
    Base {
        dir,
        nodes,
        journal,
    }
}

fn base_args(dir: &Path, journal: &Path, depth: u32) -> Vec<String> {
    let depth = depth.to_string();
    [
        "book",
        "score",
        dir.to_str().unwrap(),
        journal.to_str().unwrap(),
        "--min",
        "2",
        "--plies",
        "12",
        "--depth",
        &depth,
        "--table",
        "4096",
    ]
    .map(String::from)
    .to_vec()
}

/// The CLI's header identity for a base: everything before " depth ".
fn identity(base: &Base) -> String {
    let header = journal::read(&base.journal).unwrap().0;
    header.split(" depth ").next().unwrap().to_string()
}

fn walk_roots() -> Vec<(Key, bool)> {
    vec![(START, true), (START, false)]
}

fn params(depth: u32) -> Params {
    Params { depth, ..PARAMS }
}

fn extend_api(base: &Base, ext: &Path, rounds: u32, jobs: usize) -> Result<String, String> {
    book::extend(
        &base.nodes,
        &base.journal,
        &identity(base),
        &walk_roots(),
        &Extension { dir: ext, rounds },
        &params(PARAMS.depth),
        jobs,
    )
}

fn build_api(base: &Base, ext: &Path, rounds: u32, room: usize) -> (Vec<u8>, String) {
    book::build_extended(
        &base.nodes,
        &base.journal,
        &identity(base),
        &walk_roots(),
        &BTreeSet::from([START]),
        room,
        &Extension { dir: ext, rounds },
    )
    .unwrap()
}

fn phases(ext: &Path, rounds: u32) -> Vec<PathBuf> {
    (1..=rounds)
        .flat_map(|r| ["a", "b"].map(|p| ext.join(format!("r{r}{p}.journal"))))
        .collect()
}

/// Every node and its drop-out result over the base and the given complete
/// phase journals, recomputed with the public pieces: the test's oracle for
/// the orchestration.
fn state(base: &Base, done: &[PathBuf]) -> (BTreeMap<Key, u32>, BTreeMap<Key, Entry>) {
    let mut decoded: BTreeMap<Key, Vec<(u8, i32)>> = BTreeMap::new();
    let mut nodes = base.nodes.clone();
    for (k, line) in records(&base.journal) {
        decoded.insert(k, book::decode(k, &base.nodes, &line).unwrap());
    }
    for path in done {
        let records = records(path);
        let mut context = nodes.clone();
        context.extend(records.iter().map(|&(k, _)| (k, 0)));
        for (k, line) in &records {
            decoded.insert(*k, book::decode(*k, &context, line).unwrap());
        }
        nodes = context;
    }
    let drops: book::Drops = nodes
        .keys()
        .filter(|&&k| !is_terminal(book::board(k)))
        .map(|&k| (k, book::remaining(k, &decoded[&k], &nodes)))
        .collect();
    let entries = book::backup(&nodes, &drops).unwrap();
    (nodes, entries)
}

fn records(path: &Path) -> Vec<(Key, String)> {
    journal::read(path)
        .unwrap()
        .1
        .into_iter()
        .map(|r| {
            let (me, opp) = r.id.split_once(':').unwrap();
            let k = (
                u64::from_str_radix(me, 16).unwrap(),
                u64::from_str_radix(opp, 16).unwrap(),
            );
            (k, r.line)
        })
        .collect()
}

fn unit_ids(path: &Path) -> Vec<Key> {
    let mut keys: Vec<Key> = records(path).into_iter().map(|(k, _)| k).collect();
    keys.sort();
    keys
}

#[test]
fn extend_follows_phases() {
    let base = base("rounds", PARAMS.depth);
    let ext = base.dir.join("ext");
    extend_api(&base, &ext, 2, 2).unwrap();
    let paths = phases(&ext, 2);
    let mut done: Vec<PathBuf> = Vec::new();
    let mut sizes = Vec::new();
    for round in 0..2 {
        let (_, before) = state(&base, &done);
        let departures = book::departures(&walk_roots(), &before);
        let a = book::phase_a(&departures, &before);
        assert_eq!(unit_ids(&paths[2 * round]), a, "round {} A", round + 1);
        done.push(paths[2 * round].clone());
        let (_, after_a) = state(&base, &done);
        let b = book::phase_b(&a, &after_a);
        assert_eq!(unit_ids(&paths[2 * round + 1]), b, "round {} B", round + 1);
        done.push(paths[2 * round + 1].clone());
        sizes.push((a.len(), b.len()));
    }
    assert!(sizes[0].0 > 0 && sizes[0].1 > 0, "{sizes:?}");
    std::fs::remove_dir_all(&base.dir).unwrap();
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `{count}:{sha256}` of `text`.
fn digest(count: usize, text: &str) -> String {
    format!("{count}:{}", hex(&Sha256::digest(text.as_bytes())))
}

fn head_commit() -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[test]
fn extend_binds_inputs() {
    // Every phase header is the base header plus the registered fields, each
    // digest computed here from the sorted logical records.
    let base = base("headers", PARAMS.depth);
    let ext = base.dir.join("ext");
    extend_api(&base, &ext, 2, 2).unwrap();
    let base_header = journal::read(&base.journal).unwrap().0;
    let base_lines = sorted_lines(&base.journal);
    let roots: Vec<String> = walk_roots()
        .iter()
        .map(|&(k, e)| format!("{}:{e}", id(k)))
        .collect();
    let paths = phases(&ext, 2);
    let (mut prior, mut prior_count) = (String::new(), 0);
    for (index, path) in paths.iter().enumerate() {
        let (round, phase) = (index / 2 + 1, ["a", "b"][index % 2]);
        let lines = sorted_lines(path);
        let units: Vec<String> = unit_ids(path).iter().map(|&k| id(k)).collect();
        let (before, _) = state(&base, &paths[..index]);
        let mut context: Vec<String> = before.keys().map(|&k| id(k)).collect();
        context.extend(units.iter().cloned());
        context.sort();
        let expected = format!(
            "{base_header} extend R 2 round {round} phase {phase} base {} prior {} \
             roots {} units {} context {} extender {}",
            digest(base_lines.len(), &base_lines.join("\n")),
            digest(prior_count, &prior),
            digest(roots.len(), &roots.join(" ")),
            digest(units.len(), &units.join("\n")),
            digest(context.len(), &context.join("\n")),
            head_commit(),
        );
        assert_eq!(journal::read(path).unwrap().0, expected);
        prior.push_str(&format!("r{round}{phase}\n{}\n", lines.join("\n")));
        prior_count += lines.len();
    }
    std::fs::remove_dir_all(&base.dir).unwrap();
}

#[test]
fn extend_deterministic() {
    let base = base("deterministic", PARAMS.depth);
    let (one, three) = (base.dir.join("one"), base.dir.join("three"));
    extend_api(&base, &one, 2, 1).unwrap();
    extend_api(&base, &three, 2, 3).unwrap();
    for (a, b) in phases(&one, 2).iter().zip(phases(&three, 2)) {
        assert_eq!(sorted_lines(a), sorted_lines(&b));
        assert_eq!(journal::read(a).unwrap().0, journal::read(&b).unwrap().0);
    }
    assert_eq!(
        build_api(&base, &one, 2, 1 << 16),
        build_api(&base, &three, 2, 1 << 16)
    );
    // Re-running a complete extension scores nothing and changes nothing.
    let before: Vec<Vec<u8>> = phases(&one, 2)
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect();
    extend_api(&base, &one, 2, 2).unwrap();
    let after: Vec<Vec<u8>> = phases(&one, 2)
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect();
    assert_eq!(before, after);
    std::fs::remove_dir_all(&base.dir).unwrap();
}

#[test]
fn build_zero_rounds() {
    let base = base("legacy", PARAMS.depth);
    let ext = base.dir.join("ext");
    let legacy = book::build(
        &base.nodes,
        &base.journal,
        &identity(&base),
        &walk_roots(),
        &BTreeSet::from([START]),
        1 << 16,
    )
    .unwrap();
    // No extension directory is needed or created for R = 0.
    assert_eq!(build_api(&base, &ext, 0, 1 << 16), legacy);
    assert!(!ext.exists());
    std::fs::remove_dir_all(&base.dir).unwrap();
}

#[test]
fn build_states_rounds() {
    let base = base("partial", PARAMS.depth);
    let ext = base.dir.join("ext");
    extend_api(&base, &ext, 2, 2).unwrap();
    let (full, manifest) = build_api(&base, &ext, 2, 1 << 16);
    assert!(
        manifest.contains("# rounds requested 2 completed 2\n"),
        "{manifest}"
    );
    let (one_round, one_manifest) = build_api(&base, &ext, 1, 1 << 16);
    assert!(one_manifest.contains("# rounds requested 1 completed 1\n"));
    assert_ne!(full, one_round);
    // Per-round log lines on the artifact.
    for round in 1..=2 {
        assert!(
            manifest.contains(&format!("# round {round} departures ")),
            "{manifest}"
        );
    }
    // A partial round 2 B is ignored for the build and stated.
    let r2b = ext.join("r2b.journal");
    let (header, mut kept) = journal::read(&r2b).unwrap();
    let total = kept.len();
    assert!(total >= 2, "round 2 B too small to cut: {total}");
    kept.truncate(total / 2);
    std::fs::remove_file(&r2b).unwrap();
    let (mut journal, _) = Journal::open(&r2b, &header).unwrap();
    for record in &kept {
        journal.append(&record.id, &record.line).unwrap();
    }
    drop(journal);
    let (asset, partial) = build_api(&base, &ext, 2, 1 << 16);
    assert_eq!(asset, one_round);
    assert!(
        partial.contains("# rounds requested 2 completed 1\n"),
        "{partial}"
    );
    assert!(partial.contains(&format!(
        "# partial round 2 phase b {}/{total}\n",
        total / 2
    )));
    // An absent phase is the same as an empty partial one.
    std::fs::remove_file(&r2b).unwrap();
    let (asset, absent) = build_api(&base, &ext, 2, 1 << 16);
    assert_eq!(asset, one_round);
    assert!(absent.contains("# rounds requested 2 completed 1\n"));
    std::fs::remove_dir_all(&base.dir).unwrap();
}

#[test]
fn extend_empty_rounds() {
    // No departures: every phase journal is written empty and the rounds
    // count as complete with no change.
    let base = base("empty", PARAMS.depth);
    let ext = base.dir.join("ext");
    book::extend(
        &base.nodes,
        &base.journal,
        &identity(&base),
        &[],
        &Extension {
            dir: &ext,
            rounds: 2,
        },
        &params(PARAMS.depth),
        1,
    )
    .unwrap();
    for path in phases(&ext, 2) {
        assert!(
            journal::read(&path).unwrap().1.is_empty(),
            "{}",
            path.display()
        );
    }
    std::fs::remove_dir_all(&base.dir).unwrap();
}

#[test]
fn extend_empty_b() {
    // A non-empty A whose unit selects a book move: B has no units, its
    // journal is written empty, and the round is complete.
    let (p, square) = find(|board, square| {
        let (c, _) = book::normalize(play(board, square));
        squares(legal_moves(board.me, board.opp)).all(|s| !is_terminal(play(board, s)))
            && !is_terminal(c)
            && legal_moves(c.me, c.opp) != 0
            && squares(legal_moves(c.me, c.opp)).all(|s| {
                let child = book::normalize(play(c, s)).0;
                !is_terminal(child)
            })
    });
    let c = child_key(book::board(p), square);
    let mut nodes = node_set([p]);
    nodes.extend(node_set(
        legal(c).into_iter().map(|s| child_key(book::board(c), s)),
    ));
    assert!(!nodes.contains_key(&c));
    let dir = temp_dir("emptyb");
    let base = dir.join("base.journal");
    let header = book::header(&PARAMS, "test");
    let (mut journal, _) = Journal::open(&base, &header).unwrap();
    for &k in nodes.keys() {
        let n = token_count(k, &nodes);
        let node = book::board(k);
        // At p, the move to c is worth 500 and every other drop-out 0.
        let tokens: Vec<i32> = legal(k)
            .into_iter()
            .filter(|&s| !nodes.contains_key(&child_key(node, s)))
            .filter(|&s| !is_terminal(play(node, s)))
            .map(|s| {
                let to_c = k == p && child_key(node, s) == c;
                match (to_c, needs_pass(play(node, s))) {
                    (true, true) => 500,
                    (true, false) => -500,
                    _ => 0,
                }
            })
            .collect();
        assert_eq!(tokens.len(), n);
        let values = expected_values(k, &nodes, &tokens);
        journal
            .append(&id(k), &line_of(best_of(&values), &tokens))
            .unwrap();
    }
    drop(journal);
    let ext = dir.join("ext");
    let identity = book::identity(&PARAMS, "test");
    let run = || {
        book::extend(
            &nodes,
            &base,
            &identity,
            &[(p, true)],
            &Extension {
                dir: &ext,
                rounds: 1,
            },
            &PARAMS,
            1,
        )
    };
    run().unwrap();
    assert_eq!(unit_ids(&ext.join("r1a.journal")), vec![c]);
    let (b_header, b_records) = journal::read(&ext.join("r1b.journal")).unwrap();
    assert!(b_records.is_empty());
    assert!(b_header.contains(" units 0:"), "{b_header}");
    let built = book::build_extended(
        &nodes,
        &base,
        &identity,
        &[(p, true)],
        &BTreeSet::from([p]),
        1 << 16,
        &Extension {
            dir: &ext,
            rounds: 1,
        },
    )
    .unwrap();
    assert!(
        built.1.contains("# rounds requested 1 completed 1\n"),
        "{}",
        built.1
    );
    // Resuming a complete round with an empty B changes nothing.
    let before: Vec<Vec<u8>> = phases(&ext, 1)
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect();
    run().unwrap();
    let after: Vec<Vec<u8>> = phases(&ext, 1)
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect();
    assert_eq!(before, after);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn refuses_changed_inputs() {
    let base = base("refuse", PARAMS.depth);
    let ext = base.dir.join("ext");
    extend_api(&base, &ext, 2, 2).unwrap();
    let snapshot = |dir: &Path| -> Vec<Vec<u8>> {
        phases(dir, 2)
            .iter()
            .map(|p| std::fs::read(p).unwrap())
            .collect()
    };
    let before = snapshot(&ext);
    let refused = |result: Result<String, String>, what: &str| {
        assert!(result.is_err(), "{what} accepted");
        assert_eq!(snapshot(&ext), before, "{what} changed a journal");
    };
    // Changed roots.
    refused(
        book::extend(
            &base.nodes,
            &base.journal,
            &identity(&base),
            &[(START, true)],
            &Extension {
                dir: &ext,
                rounds: 2,
            },
            &params(PARAMS.depth),
            1,
        ),
        "changed roots",
    );
    // A different R.
    // A different R: R is in every header, so round 1 no longer matches.
    refused(extend_api(&base, &ext, 3, 1), "different R");
    std::fs::remove_dir_all(&base.dir).unwrap();
}

fn rewrite(path: &Path, header: &str, records: &[(String, String)]) {
    std::fs::remove_file(path).unwrap();
    let (mut journal, _) = Journal::open(path, header).unwrap();
    for (id, line) in records {
        journal.append(id, line).unwrap();
    }
}

fn read_all(path: &Path) -> (String, Vec<(String, String)>) {
    let (header, records) = journal::read(path).unwrap();
    (
        header,
        records.into_iter().map(|r| (r.id, r.line)).collect(),
    )
}

/// A copy of `line` with one token moved by one so that it still decodes
/// under `context` to the same `best`: a valid record with other content.
fn nudged(key: Key, context: &BTreeMap<Key, u32>, line: &str) -> Option<String> {
    let (best, tokens) = line.split_once(" | ")?;
    let tokens: Vec<i32> = tokens.split(' ').filter_map(|t| t.parse().ok()).collect();
    for i in 0..tokens.len() {
        for delta in [1, -1] {
            let mut changed = tokens.clone();
            changed[i] += delta;
            let text: Vec<String> = changed.iter().map(i32::to_string).collect();
            let candidate = format!("{best} | {}", text.join(" "));
            if book::decode(key, context, &candidate).is_ok() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Runs two rounds, applies `edit` to the base, the directory and the node
/// set, then expects both a resumed run and a build to refuse with every
/// file unchanged.
fn refused_after(name: &str, edit: impl FnOnce(&Base, &Path)) {
    let base = base(name, PARAMS.depth);
    let ext = base.dir.join("ext");
    extend_api(&base, &ext, 2, 2).unwrap();
    edit(&base, &ext);
    let files = || -> Vec<Vec<u8>> {
        std::iter::once(base.journal.clone())
            .chain(phases(&ext, 2))
            .map(|p| std::fs::read(p).unwrap_or_default())
            .collect()
    };
    let before = files();
    assert!(
        extend_api(&base, &ext, 2, 1).is_err(),
        "{name}: resume accepted"
    );
    assert!(
        book::build_extended(
            &base.nodes,
            &base.journal,
            &identity(&base),
            &walk_roots(),
            &BTreeSet::from([START]),
            1 << 16,
            &Extension {
                dir: &ext,
                rounds: 2
            },
        )
        .is_err(),
        "{name}: build accepted"
    );
    assert_eq!(files(), before, "{name}: a file changed");
    std::fs::remove_dir_all(&base.dir).unwrap();
}

#[test]
fn refuses_changed_score() {
    // A valid, still-decodable change to one round 1 A score, with the later
    // phases present: the prior digest no longer matches.
    refused_after("score", |base, ext| {
        let path = ext.join("r1a.journal");
        let (header, mut records) = read_all(&path);
        let mut context = base.nodes.clone();
        context.extend(unit_ids(&path).into_iter().map(|k| (k, 0)));
        let at = records
            .iter()
            .position(|(i, l)| nudged(key_of(i), &context, l).is_some())
            .expect("no nudgeable record");
        records[at].1 = nudged(key_of(&records[at].0), &context, &records[at].1).unwrap();
        rewrite(&path, &header, &records);
    });
}

#[test]
fn refuses_changed_base() {
    // The same for a base record: the base digest no longer matches.
    refused_after("basescore", |base, _| {
        let (header, mut records) = read_all(&base.journal);
        let at = records
            .iter()
            .position(|(i, l)| nudged(key_of(i), &base.nodes, l).is_some())
            .expect("no nudgeable record");
        records[at].1 = nudged(key_of(&records[at].0), &base.nodes, &records[at].1).unwrap();
        rewrite(&base.journal, &header, &records);
    });
}

#[test]
fn refuses_foreign_unit() {
    refused_after("foreign", |_, ext| {
        let path = ext.join("r1a.journal");
        let (header, mut records) = read_all(&path);
        records.push((id(START), "none | ".into()));
        rewrite(&path, &header, &records);
    });
}

#[test]
fn refuses_duplicate_unit() {
    refused_after("duplicate", |_, ext| {
        let path = ext.join("r1b.journal");
        let (header, mut records) = read_all(&path);
        records.push(records[0].clone());
        rewrite(&path, &header, &records);
    });
}

#[test]
fn refuses_changed_header() {
    refused_after("header", |_, ext| {
        let path = ext.join("r1b.journal");
        let (header, records) = read_all(&path);
        let at = header.find(" units ").unwrap() + " units ".len();
        let changed = format!("{}9{}", &header[..at], &header[at + 1..]);
        rewrite(&path, &changed, &records);
    });
}

fn key_of(text: &str) -> Key {
    let (me, opp) = text.split_once(':').unwrap();
    (
        u64::from_str_radix(me, 16).unwrap(),
        u64::from_str_radix(opp, 16).unwrap(),
    )
}

/// A fresh base with one header field replaced (or none), then one run
/// with `params`: refused before any phase journal exists, or accepted.
fn policy_case(name: &str, field: Option<(&str, &str)>, params: Params) -> bool {
    let base = base(name, PARAMS.depth);
    if let Some((from, to)) = field {
        let (header, records) = read_all(&base.journal);
        assert!(header.contains(from), "{from} not in {header}");
        rewrite(&base.journal, &header.replacen(from, to, 1), &records);
    }
    let ext = base.dir.join("ext");
    let result = book::extend(
        &base.nodes,
        &base.journal,
        &identity(&base),
        &walk_roots(),
        &Extension {
            dir: &ext,
            rounds: 1,
        },
        &params,
        1,
    );
    let created = ext.join("r1a.journal").exists();
    std::fs::remove_dir_all(&base.dir).unwrap();
    assert_eq!(result.is_err(), !created, "{name}: {result:?}");
    result.is_err()
}

#[test]
fn refuses_other_policy() {
    // Each case changes one input on an otherwise valid base.
    assert!(!policy_case("control", None, PARAMS));
    assert!(policy_case(
        "threshold",
        Some((" threshold 18 ", " threshold 17 ")),
        PARAMS
    ));
    let eval = {
        let header = book::header(&PARAMS, "test");
        let at = header.find(" eval ").unwrap() + " eval ".len();
        header[at..at + 64].to_string()
    };
    let other = if eval.starts_with('0') { "1" } else { "0" }.repeat(64);
    assert!(policy_case("eval", Some((&eval, &other)), PARAMS));
    assert!(policy_case(
        "depth",
        None,
        Params {
            depth: PARAMS.depth + 1,
            ..PARAMS
        }
    ));
    assert!(policy_case(
        "table",
        None,
        Params {
            table: PARAMS.table / 2,
            ..PARAMS
        }
    ));
}

#[test]
fn overflow_extension_first() {
    let base = base("overflow", PARAMS.depth);
    let ext = base.dir.join("ext");
    extend_api(&base, &ext, 2, 2).unwrap();
    let (_, full) = build_api(&base, &ext, 2, 1 << 16);
    let total = full.lines().filter(|l| !l.starts_with('#')).count();
    let (_, legacy) = build_api(&base, &ext, 0, 1 << 16);
    let legacy_records = legacy.lines().filter(|l| !l.starts_with('#')).count();
    assert!(total > legacy_records, "extension added no records");
    // Room for one fewer record than the extension needs: the removals are
    // count-0 extension nodes, never base nodes or the start.
    let (_, cut) = build_api(&base, &ext, 2, (total - 1) * 11);
    let removed: Vec<Key> = cut
        .lines()
        .filter_map(|l| l.strip_prefix("# removed "))
        .map(|l| {
            let text = l.split(' ').next().unwrap();
            let (me, opp) = text.split_once(':').unwrap();
            (
                u64::from_str_radix(me, 16).unwrap(),
                u64::from_str_radix(opp, 16).unwrap(),
            )
        })
        .collect();
    assert!(!removed.is_empty());
    assert!(
        removed.iter().all(|k| !base.nodes.contains_key(k)),
        "{removed:?}"
    );
    std::fs::remove_dir_all(&base.dir).unwrap();
}

// ---- Interruption, through the CLI ----------------------------------------

/// Kills its child on drop, so a failed assertion never leaves one running.
struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn archive(games: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0; 16];
    bytes[4..8].copy_from_slice(&(games.len() as u32).to_le_bytes());
    bytes[10..12].copy_from_slice(&2025u16.to_le_bytes());
    bytes[12] = 8;
    bytes[14] = 24;
    for game in games {
        let mut record = [0u8; 68];
        for (index, &square) in game.iter().enumerate() {
            record[8 + index] = (square / 8 + 1) * 10 + square % 8 + 1;
        }
        bytes.extend_from_slice(&record);
    }
    bytes
}

fn trainer(args: &[String]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
        .args(args)
        .output()
        .unwrap()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn extend_args(base: &Base, ext: &Path, rounds: u32, depth: u32, jobs: usize) -> Vec<String> {
    let roots = base.dir.join("roots.txt");
    std::fs::write(&roots, "# no gate roots\n").unwrap();
    [
        "book".to_string(),
        "extend".into(),
        base.dir.to_str().unwrap().into(),
        base.journal.to_str().unwrap().into(),
        "--extension".into(),
        ext.to_str().unwrap().into(),
        "--rounds".into(),
        rounds.to_string(),
        "--roots".into(),
        roots.to_str().unwrap().into(),
        "--min".into(),
        "2".into(),
        "--plies".into(),
        "12".into(),
        "--depth".into(),
        depth.to_string(),
        "--table".into(),
        "4096".into(),
        "--jobs".into(),
        jobs.to_string(),
    ]
    .to_vec()
}

fn build_args(base: &Base, ext: &Path, rounds: u32, out: &str) -> Vec<String> {
    let roots = base.dir.join("roots.txt");
    [
        "book".to_string(),
        "build".into(),
        base.dir.to_str().unwrap().into(),
        base.journal.to_str().unwrap().into(),
        "--roots".into(),
        roots.to_str().unwrap().into(),
        "--asset".into(),
        base.dir.join(format!("{out}.bin")).to_str().unwrap().into(),
        "--manifest".into(),
        base.dir.join(format!("{out}.txt")).to_str().unwrap().into(),
        "--min".into(),
        "2".into(),
        "--plies".into(),
        "12".into(),
        "--extension".into(),
        ext.to_str().unwrap().into(),
        "--rounds".into(),
        rounds.to_string(),
    ]
    .to_vec()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, to.join(path.file_name().unwrap())).unwrap();
    }
}

/// Spawns `args`, waits until `ready` holds, then kills it. True when the
/// kill was a SIGKILL of a still-running process.
fn kill_when(args: &[String], ready: impl Fn() -> bool) -> bool {
    let mut child = Running(
        Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    while !ready() {
        if child.0.try_wait().unwrap().is_some() {
            return false;
        }
        assert!(
            start.elapsed() < Duration::from_secs(600),
            "kill point not reached"
        );
        sleep(Duration::from_millis(1));
    }
    child.0.kill().unwrap();
    child.0.wait().unwrap().signal() == Some(9)
}

fn count(path: &Path) -> usize {
    journal::read(path).map_or(0, |(_, r)| r.len())
}

#[test]
fn extend_resume_kill() {
    // Live SIGKILLs inside round 1 A and inside round 1 B, deepening until
    // both land with the phase incomplete; then resume with other jobs.
    let mut chosen = None;
    for depth in [4, 6, 8, 10] {
        let base = base(&format!("kill{depth}"), depth);
        let full = base.dir.join("full");
        let output = trainer(&extend_args(&base, &full, 2, depth, 1));
        assert!(output.status.success(), "{}", stderr(&output));
        let (a_total, b_total) = (
            count(&full.join("r1a.journal")),
            count(&full.join("r1b.journal")),
        );
        let in_a = base.dir.join("in_a");
        let a_ok = a_total >= 2
            && kill_when(&extend_args(&base, &in_a, 2, depth, 1), || {
                count(&in_a.join("r1a.journal")) >= 1
            })
            && count(&in_a.join("r1a.journal")) < a_total;
        let in_b = base.dir.join("in_b");
        let b_ok = b_total >= 2
            && kill_when(&extend_args(&base, &in_b, 2, depth, 1), || {
                count(&in_b.join("r1b.journal")) >= 1
            })
            && count(&in_b.join("r1b.journal")) < b_total
            && !in_b.join("r2a.journal").exists();
        if a_ok && b_ok {
            chosen = Some((base, depth));
            break;
        }
        std::fs::remove_dir_all(&base.dir).unwrap();
    }
    let (base, depth) = chosen.expect("no depth gave kills inside both A and B");
    let full = base.dir.join("full");
    let expected: Vec<Vec<String>> = phases(&full, 2).iter().map(|p| sorted_lines(p)).collect();

    // Replay fixtures, not live kills: the boundary after A, and a torn
    // round 1 B record.
    let between = base.dir.join("between");
    copy_dir(&full, &between);
    for path in phases(&between, 2).iter().skip(1) {
        std::fs::remove_file(path).unwrap();
    }
    let torn = base.dir.join("torn");
    copy_dir(&full, &torn);
    for path in phases(&torn, 2).iter().skip(2) {
        std::fs::remove_file(path).unwrap();
    }
    let r1b = torn.join("r1b.journal");
    let text = std::fs::read(&r1b).unwrap();
    let ends: Vec<usize> = text
        .iter()
        .enumerate()
        .filter(|&(_, &b)| b == b'\n')
        .map(|(i, _)| i)
        .collect();
    let cut = ends[ends.len() / 2] + 1;
    let mut bytes = text[..cut].to_vec();
    bytes.extend_from_slice(&text[cut..cut + (ends[ends.len() / 2 + 1] - cut) / 2]);
    std::fs::write(&r1b, bytes).unwrap();

    let built = |dir: &Path, out: &str| {
        let output = trainer(&build_args(&base, dir, 2, out));
        assert!(output.status.success(), "{}", stderr(&output));
        (
            std::fs::read(base.dir.join(format!("{out}.bin"))).unwrap(),
            std::fs::read(base.dir.join(format!("{out}.txt"))).unwrap(),
        )
    };
    let reference = built(&full, "full");
    for name in ["in_a", "in_b", "between", "torn"] {
        let dir = base.dir.join(name);
        let output = trainer(&extend_args(&base, &dir, 2, depth, 3));
        assert!(output.status.success(), "{name}: {}", stderr(&output));
        let got: Vec<Vec<String>> = phases(&dir, 2).iter().map(|p| sorted_lines(p)).collect();
        assert_eq!(got, expected, "{name}");
        assert_eq!(built(&dir, name), reference, "{name}");
    }
    std::fs::remove_dir_all(&base.dir).unwrap();
}

#[test]
fn extend_cli_options() {
    // Each refusal names its option, before any input is read.
    let refused = |args: &[&str], want: &str| {
        let mut all = vec!["book", "extend", "missing-dir", "missing.journal"];
        all.extend_from_slice(args);
        let output = trainer(&all.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert!(!output.status.success(), "{args:?} accepted");
        assert!(
            stderr(&output).contains(want),
            "{args:?}: {}",
            stderr(&output)
        );
    };
    refused(
        &["--rounds", "3", "--roots", "r"],
        "--extension is required",
    );
    refused(
        &["--extension", "e", "--roots", "r"],
        "--rounds is required",
    );
    refused(
        &["--extension", "e", "--rounds", "3"],
        "--roots is required",
    );
    refused(
        &[
            "--extension",
            "e",
            "--rounds",
            "3",
            "--roots",
            "r",
            "--sample",
            "2",
        ],
        "option --sample not valid for book extend",
    );
    refused(
        &["--extension", "e", "--rounds", "x", "--roots", "r"],
        "invalid --rounds x",
    );
    // Build accepts --extension only with --rounds, and the reverse.
    let build = |args: &[&str], want: &str| {
        let mut all = vec![
            "book",
            "build",
            "missing-dir",
            "missing.journal",
            "--roots",
            "r",
            "--asset",
            "a",
            "--manifest",
            "m",
        ];
        all.extend_from_slice(args);
        let output = trainer(&all.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert!(!output.status.success(), "{args:?} accepted");
        assert!(
            stderr(&output).contains(want),
            "{args:?}: {}",
            stderr(&output)
        );
    };
    build(&["--extension", "e"], "--rounds is required");
    build(&["--rounds", "2"], "--extension is required");
}
