use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use othello::board::{Board, final_score, is_terminal, legal_moves, pass, play, squares};
use othello::rng::Rng;
use othello::score::terminal_score;
use othello::search::{ProbCut, SearchConfig};
use othello_tools::sym::canonical;
use othello_trainer::book::{self, Entry, Key, Params};

/// Random legal games of up to `plies` placements; the first `shared`
/// placements depend only on `game % families`, so prefixes repeat.
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

/// The first (board, square) in random playouts where `want` holds.
fn find(want: impl Fn(Board, u8) -> bool) -> (Board, u8) {
    for seed in 1..20_000 {
        let mut rng = Rng::new(seed);
        let mut board = Board::START;
        while !is_terminal(board) {
            if legal_moves(board.me, board.opp) == 0 {
                board = pass(board);
            }
            let legal: Vec<u8> = squares(legal_moves(board.me, board.opp)).collect();
            for &square in &legal {
                if want(board, square) {
                    return (board, square);
                }
            }
            board = play(board, legal[(rng.next_u64() % legal.len() as u64) as usize]);
        }
    }
    panic!("no fixture found");
}

fn needs_pass(board: Board) -> bool {
    legal_moves(board.me, board.opp) == 0 && !is_terminal(board)
}

/// A search stand-in: refuses boards without a move, and records calls.
fn recorder(calls: &RefCell<Vec<(Board, u32)>>) -> impl FnMut(Board, u32) -> i32 + '_ {
    move |board, depth| {
        assert_ne!(
            legal_moves(board.me, board.opp),
            0,
            "search on a board without a move"
        );
        calls.borrow_mut().push((board, depth));
        (board.me.count_ones() as i32 - board.opp.count_ones() as i32) * 100 + depth as i32
    }
}

fn node_set(keys: impl IntoIterator<Item = Key>) -> BTreeMap<Key, u32> {
    keys.into_iter().map(|key| (key, 10)).collect()
}

fn child_key(board: Board, square: u8) -> Key {
    book::key(book::normalize(play(board, square)).0)
}

const START: Key = (Board::START.me, Board::START.opp);

#[test]
fn key_is_canonical() {
    let board = play(Board::START, 19);
    assert_eq!(book::key(board), canonical(board));
    assert_eq!(book::key(book::board(book::key(board))), book::key(board));
}

#[test]
fn count_once_per_game() {
    // Three identical games: every position counts once per game.
    let mut list = games(2, 1, 4, 4);
    list.push(list[0].clone());
    let counts = book::count(&list, 3);
    assert_eq!(counts[&book::key(Board::START)], 3);
    let after_one = book::key(play(Board::START, list[0][0]));
    assert_eq!(counts[&after_one], 3);
    assert!(counts.keys().all(|&key| {
        let board = book::board(key);
        legal_moves(board.me, board.opp) != 0 || is_terminal(board)
    }));
}

#[test]
fn count_plies_are_placements() {
    // Ply limit 3 counts the start and the positions after 1 and 2 placements.
    let list = games(1, 1, 0, 10);
    let counts = book::count(&list, 3);
    assert_eq!(counts.len(), 3);
    let mut board = Board::START;
    for &square in &list[0][..2] {
        board = book::normalize(play(board, square)).0;
        assert!(counts.contains_key(&book::key(board)));
    }
}

#[test]
fn count_contracts_passes() {
    // A position whose mover must pass is counted as the passed position,
    // at the same ply, never as itself.
    let (board, square) = find(|board, square| needs_pass(play(board, square)));
    let child = play(board, square);
    let (normal, passed) = book::normalize(child);
    assert!(passed);
    assert_eq!(normal, pass(child));
    let (same, passed) = book::normalize(board);
    assert!(!passed);
    assert_eq!(same, board);
}

#[test]
fn partition_counts_unplayed_edges() {
    // All four first moves reach the same canonical child. With that child
    // in the node set every one is a book move, though no game played them.
    let child = book::key(play(Board::START, 19));
    let nodes = node_set([START, child]);
    let moves = book::partition(START, &nodes);
    let found: Vec<u8> = moves.iter().map(|m| m.square).collect();
    assert_eq!(
        found,
        squares(legal_moves(Board::START.me, Board::START.opp)).collect::<Vec<_>>()
    );
    assert!(
        moves
            .iter()
            .all(|m| m.book && m.child == child && !m.passed)
    );
}

#[test]
fn partition_is_complete() {
    let list = games(40, 4, 6, 12);
    let counts = book::count(&list, 12);
    let nodes: BTreeMap<Key, u32> = counts.into_iter().filter(|&(_, n)| n >= 2).collect();
    for &key in nodes.keys() {
        let board = book::board(key);
        let moves = book::partition(key, &nodes);
        let legal: Vec<u8> = squares(legal_moves(board.me, board.opp)).collect();
        assert_eq!(moves.iter().map(|m| m.square).collect::<Vec<_>>(), legal);
        for m in &moves {
            assert_eq!(m.child, child_key(board, m.square));
            assert_eq!(m.book, nodes.contains_key(&m.child));
            assert_eq!(m.passed, needs_pass(play(board, m.square)));
        }
    }
}

#[test]
fn value_terminal() {
    let (board, square) = find(|board, square| is_terminal(play(board, square)));
    let child = play(board, square);
    let calls = RefCell::new(Vec::new());
    let value = book::value(child, 7, &mut recorder(&calls));
    assert_eq!(value, terminal_score(final_score(child)));
    assert!(calls.borrow().is_empty());
}

#[test]
fn value_terminal_awards_empties() {
    // 10 discs to none, 54 empty: the winner takes the empties.
    let board = Board { me: 0x3ff, opp: 0 };
    let calls = RefCell::new(Vec::new());
    assert_eq!(book::value(board, 3, &mut recorder(&calls)), 6400);
}

#[test]
fn value_pass_same_depth() {
    let (board, square) = find(|board, square| needs_pass(play(board, square)));
    let child = play(board, square);
    let calls = RefCell::new(Vec::new());
    let value = book::value(child, 5, &mut recorder(&calls));
    let calls = calls.into_inner();
    assert_eq!(calls, vec![(pass(child), 5)]);
    let spare = RefCell::new(Vec::new());
    assert_eq!(value, -recorder(&spare)(pass(child), 5));
}

#[test]
fn dropout_order_and_depth() {
    // Nothing in the book: every legal move is a drop-out, searched in
    // ascending square order at one less depth.
    let board = play(play(Board::START, 19), 18);
    let key = book::key(board);
    let calls = RefCell::new(Vec::new());
    let (value, square) = book::dropout(key, &node_set([key]), 6, &mut recorder(&calls)).unwrap();
    let canonical_board = book::board(key);
    let legal: Vec<u8> = squares(legal_moves(canonical_board.me, canonical_board.opp)).collect();
    let calls = calls.into_inner();
    assert_eq!(calls.len(), legal.len());
    for (call, &square) in calls.iter().zip(&legal) {
        let child = play(canonical_board, square);
        let expected = if needs_pass(child) {
            (pass(child), 5)
        } else {
            (child, 5)
        };
        assert_eq!(*call, expected);
    }
    let spare = RefCell::new(Vec::new());
    let mut again = recorder(&spare);
    let values: Vec<i32> = legal
        .iter()
        .map(|&s| -book::value(play(canonical_board, s), 5, &mut again))
        .collect();
    let best = *values.iter().max().unwrap();
    assert_eq!(value, best);
    assert_eq!(
        square,
        legal[values.iter().position(|&v| v == best).unwrap()]
    );
}

#[test]
fn dropout_skips_book_moves() {
    let child = book::key(play(Board::START, 19));
    let calls = RefCell::new(Vec::new());
    let found = book::dropout(START, &node_set([START, child]), 6, &mut recorder(&calls));
    assert_eq!(found, None);
    assert!(calls.borrow().is_empty());
}

/// Nodes: the start, its one canonical child, and every canonical child of
/// that, with hand-set drop-out results.
fn small_book() -> (BTreeMap<Key, u32>, BTreeMap<Key, Option<(i32, u8)>>) {
    let one = book::key(play(Board::START, 19));
    let one_board = book::board(one);
    let twos: BTreeSet<Key> = squares(legal_moves(one_board.me, one_board.opp))
        .map(|s| child_key(one_board, s))
        .collect();
    let mut nodes = node_set([START, one]);
    nodes.extend(node_set(twos.iter().copied()));
    let mut drops = BTreeMap::new();
    drops.insert(START, None);
    drops.insert(one, None);
    for (index, &two) in twos.iter().enumerate() {
        let board = book::board(two);
        let first = squares(legal_moves(board.me, board.opp)).next().unwrap();
        drops.insert(two, Some((100 * index as i32 - 50, first)));
    }
    (nodes, drops)
}

#[test]
fn backup_negamax() {
    let (nodes, drops) = small_book();
    let entries = book::backup(&nodes, &drops).unwrap();
    let one = book::key(play(Board::START, 19));
    let one_board = book::board(one);
    let best = squares(legal_moves(one_board.me, one_board.opp))
        .map(|s| (-entries[&child_key(one_board, s)].value, s))
        .max_by_key(|&(v, s)| (v, std::cmp::Reverse(s)))
        .unwrap();
    assert_eq!(
        entries[&one],
        Entry {
            value: best.0,
            square: Some(best.1),
            book: true
        }
    );
    assert_eq!(entries[&START].value, -entries[&one].value);
    assert_eq!(entries[&START].square, Some(19));
    for (&key, drop) in &drops {
        if let Some((value, square)) = drop {
            assert_eq!(
                entries[&key],
                Entry {
                    value: *value,
                    square: Some(*square),
                    book: false
                }
            );
        }
    }
}

#[test]
fn backup_ties() {
    // Equal values: the lower square wins, whichever kind of move it is.
    let one = book::key(play(Board::START, 19));
    let one_board = book::board(one);
    let legal: Vec<u8> = squares(legal_moves(one_board.me, one_board.opp)).collect();
    let kids: Vec<Key> = legal.iter().map(|&s| child_key(one_board, s)).collect();
    let mut nodes = node_set([one]);
    let mut drops = BTreeMap::new();
    // The highest square is a book move worth 0; a drop-out on the lowest
    // square is also worth 0, and wins on square order.
    let last = *kids.last().unwrap();
    nodes.insert(last, 10);
    let last_board = book::board(last);
    let any = squares(legal_moves(last_board.me, last_board.opp))
        .next()
        .unwrap();
    drops.insert(last, Some((0, any)));
    drops.insert(one, Some((0, legal[0])));
    let entry = book::backup(&nodes, &drops).unwrap()[&one];
    assert_eq!(
        entry,
        Entry {
            value: 0,
            square: Some(legal[0]),
            book: false
        }
    );
    // A book move on the lowest square ties a drop-out on a higher square.
    drops.insert(one, Some((0, legal[1])));
    let mut nodes = nodes;
    let low = kids[0];
    nodes.insert(low, 10);
    let low_board = book::board(low);
    let any = squares(legal_moves(low_board.me, low_board.opp))
        .next()
        .unwrap();
    drops.insert(low, Some((0, any)));
    let entry = book::backup(&nodes, &drops).unwrap()[&one];
    assert_eq!(
        entry,
        Entry {
            value: 0,
            square: Some(legal[0]),
            book: true
        }
    );
}

#[test]
fn backup_terminal_and_pass() {
    // A terminal node takes its exact score and no move; a book move whose
    // child needs a pass takes the passed node's value without a sign change.
    let (board, square) = find(|board, square| {
        let child = play(board, square);
        needs_pass(child) && !is_terminal(pass(child))
    });
    let parent = book::key(board);
    let moves = book::partition(parent, &node_set([parent]));
    let passing = moves.iter().find(|m| m.passed).expect("a passing move");
    let mut nodes = node_set([parent, passing.child]);
    let moves = book::partition(parent, &nodes);
    let mut drops = BTreeMap::new();
    let child_board = book::board(passing.child);
    let any = squares(legal_moves(child_board.me, child_board.opp))
        .next()
        .unwrap();
    drops.insert(passing.child, Some((-700, any)));
    let drop = moves.iter().find(|m| !m.book).map(|m| (-9000, m.square));
    drops.insert(parent, drop);
    let entries = book::backup(&nodes, &drops).unwrap();
    assert_eq!(
        entries[&parent],
        Entry {
            value: -700,
            square: Some(passing.square),
            book: true
        }
    );
    let _ = square;

    let (board, square) = find(|board, square| is_terminal(play(board, square)));
    let end = book::key(play(board, square));
    nodes.insert(end, 10);
    let entries = book::backup(&nodes, &drops).unwrap();
    let end_board = book::board(end);
    assert_eq!(
        entries[&end],
        Entry {
            value: terminal_score(final_score(end_board)),
            square: None,
            book: false
        }
    );
}

#[test]
fn backup_needs_dropouts() {
    let (nodes, mut drops) = small_book();
    let missing = *drops.keys().last().unwrap();
    drops.remove(&missing);
    assert!(book::backup(&nodes, &drops).is_err());
}

#[test]
fn reachable_follows_policy() {
    let (nodes, drops) = small_book();
    let entries = book::backup(&nodes, &drops).unwrap();
    let one = book::key(play(Board::START, 19));
    // Engine to move at the start: the start record, then every reply.
    let records = book::reachable(&[(START, true)], &entries, &BTreeSet::new());
    let one_board = book::board(one);
    let replies: BTreeSet<Key> = squares(legal_moves(one_board.me, one_board.opp))
        .map(|s| child_key(one_board, s))
        .collect();
    let mut expected: BTreeSet<Key> = replies.clone();
    expected.insert(START);
    assert_eq!(records, expected);
    // Engine second: only the engine's move at `one` is stored.
    let records = book::reachable(&[(START, false)], &entries, &BTreeSet::new());
    assert_eq!(records, BTreeSet::from([one]));
}

#[test]
fn reachable_removed_key() {
    // A removed key stops the walk where the engine is to move, but an
    // opponent visit to the same key continues.
    let (nodes, drops) = small_book();
    let entries = book::backup(&nodes, &drops).unwrap();
    let one = book::key(play(Board::START, 19));
    let removed = BTreeSet::from([one]);
    assert!(book::reachable(&[(START, false)], &entries, &removed).is_empty());
    let records = book::reachable(&[(START, true)], &entries, &removed);
    assert!(records.contains(&START) && !records.contains(&one) && records.len() > 1);
}

#[test]
fn reachable_colour_through_pass() {
    // After a passing book move the same side moves again.
    let (board, _) = find(|board, square| {
        let child = play(board, square);
        needs_pass(child) && !is_terminal(pass(child))
    });
    let parent = book::key(board);
    let moves = book::partition(parent, &node_set([parent]));
    let passing = *moves.iter().find(|m| m.passed).unwrap();
    let nodes = node_set([parent, passing.child]);
    let moves = book::partition(parent, &nodes);
    let child_board = book::board(passing.child);
    let any = squares(legal_moves(child_board.me, child_board.opp))
        .next()
        .unwrap();
    let mut drops = BTreeMap::new();
    drops.insert(passing.child, Some((-700, any)));
    let drop = moves.iter().find(|m| !m.book).map(|m| (-9000, m.square));
    drops.insert(parent, drop);
    let entries = book::backup(&nodes, &drops).unwrap();
    let records = book::reachable(&[(parent, true)], &entries, &BTreeSet::new());
    assert_eq!(records, BTreeSet::from([parent, passing.child]));
}

#[test]
fn reachable_skips_terminal() {
    let (board, square) = find(|board, square| is_terminal(play(board, square)));
    let end = book::key(play(board, square));
    let entries = BTreeMap::from([(
        end,
        Entry {
            value: 0,
            square: None,
            book: false,
        },
    )]);
    assert!(book::reachable(&[(end, true)], &entries, &BTreeSet::new()).is_empty());
}

#[test]
fn truncate_order_and_roots() {
    let (nodes, drops) = small_book();
    let entries = book::backup(&nodes, &drops).unwrap();
    let roots = [(START, true)];
    let all = book::reachable(&roots, &entries, &BTreeSet::new());
    let mut counts: BTreeMap<Key, u32> = all.iter().map(|&k| (k, 50)).collect();
    // The start is the rarest but mandatory; the next rarest goes first.
    counts.insert(START, 1);
    let victim = *all.iter().find(|&&k| k != START).unwrap();
    counts.insert(victim, 2);
    let mandatory = BTreeSet::from([START]);
    let kept = book::truncate(&roots, &entries, &counts, &mandatory, all.len() - 1).unwrap();
    assert_eq!(kept.len(), all.len() - 1);
    assert!(kept.contains(&START) && !kept.contains(&victim));
    // Equal counts fall back to the zobrist key, ascending.
    let flat: BTreeMap<Key, u32> = all.iter().map(|&k| (k, 50)).collect();
    let kept = book::truncate(&roots, &entries, &flat, &mandatory, all.len() - 1).unwrap();
    let first = all
        .iter()
        .filter(|&&k| k != START)
        .min_by_key(|&&k| othello::tt::zobrist(book::board(k)))
        .unwrap();
    assert!(!kept.contains(first));
}

#[test]
fn truncate_keeps_retained_move() {
    // Removing a child keeps the parent's stored move; the child is gone.
    let (nodes, drops) = small_book();
    let entries = book::backup(&nodes, &drops).unwrap();
    let roots = [(START, true)];
    let all = book::reachable(&roots, &entries, &BTreeSet::new());
    let counts: BTreeMap<Key, u32> = all
        .iter()
        .map(|&k| (k, if k == START { 100 } else { 1 }))
        .collect();
    let kept = book::truncate(&roots, &entries, &counts, &BTreeSet::from([START]), 1).unwrap();
    assert_eq!(kept, BTreeSet::from([START]));
    assert_eq!(entries[&START].square, Some(19));
}

#[test]
fn truncate_mandatory_overflow() {
    let (nodes, drops) = small_book();
    let entries = book::backup(&nodes, &drops).unwrap();
    let roots = [(START, true)];
    let all = book::reachable(&roots, &entries, &BTreeSet::new());
    let counts: BTreeMap<Key, u32> = all.iter().map(|&k| (k, 5)).collect();
    let mandatory: BTreeSet<Key> = all.iter().copied().take(2).collect();
    assert!(book::truncate(&roots, &entries, &counts, &mandatory, 1).is_err());
}

#[test]
fn encode_records() {
    let bytes = book::encode(&[(9, 3, 40_000), (2, 63, -12)]).unwrap();
    assert_eq!(bytes.len(), 22);
    assert_eq!(&bytes[..8], &2u64.to_le_bytes());
    assert_eq!(bytes[8], 63);
    assert_eq!(&bytes[9..11], &(-12i16).to_le_bytes());
    assert_eq!(&bytes[11..19], &9u64.to_le_bytes());
    assert_eq!(&bytes[20..22], &i16::MAX.to_le_bytes());
    assert!(book::encode(&[(5, 1, 0), (5, 2, 0)]).is_err());
    assert!(book::encode(&[(5, 64, 0)]).is_err());
    assert_eq!(book::encode(&[]).unwrap(), Vec::<u8>::new());
}

#[test]
fn shuffle_permutes() {
    let mut a: Vec<u32> = (0..50).collect();
    let mut b = a.clone();
    book::shuffle(&mut a, 150);
    book::shuffle(&mut b, 150);
    assert_eq!(a, b);
    assert_ne!(a, (0..50).collect::<Vec<_>>());
    let mut sorted = a.clone();
    sorted.sort();
    assert_eq!(sorted, (0..50).collect::<Vec<_>>());
}

#[test]
fn roots_frozen() {
    let list = games(60, 6, 8, 12);
    let counts = book::count(&list, 12);
    let nodes: BTreeMap<Key, u32> = counts.into_iter().filter(|&(_, n)| n >= 2).collect();
    let roots = book::roots(&list, &nodes, 6..=8, 5, 151).unwrap();
    assert_eq!(roots, book::roots(&list, &nodes, 6..=8, 5, 151).unwrap());
    let mut seen = BTreeSet::new();
    for line in &roots {
        assert!((6..=8).contains(&line.len()));
        let board = line
            .iter()
            .fold(Board::START, |b, &s| book::normalize(play(b, s)).0);
        assert!(legal_moves(board.me, board.opp) != 0);
        assert!(nodes.contains_key(&book::key(board)));
        assert!(seen.insert(book::key(board)));
    }
    assert!(book::roots(&list, &nodes, 6..=8, 10_000, 151).is_err());
}

#[test]
fn projection_and_rule() {
    let times = [1.0, 2.0, 3.0, 10.0];
    let (mean, upper) = book::projection(&times, 1000, 5, 150);
    assert!((mean - 4.0 * 1000.0 / 5.0 / 3600.0).abs() < 1e-9);
    assert!(upper >= mean);
    assert_eq!((mean, upper), book::projection(&times, 1000, 5, 150));
    assert_eq!(
        book::depth_rule(&[(18, 10.0), (19, 30.0), (20, 40.0)], 36.0),
        Some(19)
    );
    assert_eq!(
        book::depth_rule(&[(18, 37.0), (19, 50.0), (20, 90.0)], 36.0),
        None
    );
}

#[test]
fn policy_explicit() {
    let config = book::policy();
    assert_eq!(config.endgame_threshold, 18);
    assert!(config.ordering);
    assert_eq!(config.probcut, Some(ProbCut::FITTED));
    let board = play(Board::START, 19);
    assert_eq!((config.eval)(board), othello::pattern::evaluate(board));
    let _: SearchConfig = config;
}

#[test]
fn header_names_inputs() {
    let params = Params {
        min: 10,
        plies: 30,
        depth: 20,
        table: 1 << 22,
    };
    let header = book::header(&params, "abc123");
    for part in [
        "book v1",
        "min 10",
        "plies 30",
        "depth 20",
        "table 4194304",
        "input abc123",
        "eval ",
        "engine ",
        "threshold 18",
        "ordering true",
    ] {
        assert!(header.contains(part), "{part} missing from {header}");
    }
    assert!(header.contains(&format!("{:?}", ProbCut::FITTED)));
    let other = book::header(
        &Params {
            depth: 19,
            ..params
        },
        "abc123",
    );
    assert_ne!(header, other);
}

/// Scores every node of a small archive at depth 2 and builds the asset.
fn build_with(
    dir: &Path,
    name: &str,
    nodes: &[Key],
    jobs: usize,
) -> (Vec<String>, Vec<u8>, String) {
    let params = Params {
        min: 2,
        plies: 8,
        depth: 2,
        table: 1 << 12,
    };
    let journal = dir.join(format!("{name}.journal"));
    book::score(nodes, &params, jobs, &journal, "header test").unwrap();
    let list = games(30, 3, 6, 10);
    let all = book::count(&list, params.plies);
    let node_map: BTreeMap<Key, u32> = all.into_iter().filter(|&(_, n)| n >= params.min).collect();
    let (asset, manifest) = book::build(
        &node_map,
        &journal,
        &[(START, true), (START, false)],
        &BTreeSet::from([START]),
        1 << 16,
    )
    .unwrap();
    (sorted_lines(&journal), asset, manifest)
}

#[test]
fn score_deterministic() {
    let dir = temp_dir("deterministic");
    let list = games(30, 3, 6, 10);
    let counts = book::count(&list, 8);
    let nodes: Vec<Key> = counts
        .iter()
        .filter(|&(_, &n)| n >= 2)
        .map(|(&k, _)| k)
        .collect();
    let mut reversed = nodes.clone();
    reversed.reverse();
    let one = build_with(&dir, "one", &nodes, 1);
    assert_eq!(one, build_with(&dir, "three", &nodes, 3));
    assert_eq!(one, build_with(&dir, "reversed", &reversed, 2));
    assert_eq!(one.1.len() % 11, 0);
    assert_eq!(
        one.2.lines().filter(|l| !l.starts_with('#')).count(),
        one.1.len() / 11
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn score_resume() {
    let dir = temp_dir("resume");
    std::fs::write(dir.join("games.wtb"), archive(&games(30, 3, 10, 40))).unwrap();
    let dir_arg = dir.to_str().unwrap();
    let args = |journal: &str, jobs: &str| -> Vec<String> {
        [
            "book", "score", dir_arg, journal, "--min", "2", "--plies", "12", "--depth", "6",
            "--jobs", jobs,
        ]
        .map(String::from)
        .to_vec()
    };
    let full = dir.join("full.journal");
    let output = trainer(&args(full.to_str().unwrap(), "1"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = sorted_lines(&full);
    assert!(expected.len() > 3);

    let killed = dir.join("killed.journal");
    let mut child = Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
        .args(args(killed.to_str().unwrap(), "1"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Ok((_, records)) = othello_tools::journal::read(&killed)
            && !records.is_empty()
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "no journal line before timeout"
        );
        sleep(Duration::from_millis(5));
    }
    let _ = child.kill();
    assert_eq!(child.wait().unwrap().signal(), Some(9));
    assert!(sorted_lines(&killed).len() < expected.len());
    let output = trainer(&args(killed.to_str().unwrap(), "3"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(sorted_lines(&killed), expected);

    let mut changed = args(full.to_str().unwrap(), "1");
    changed[9] = "5".into();
    let output = trainer(&changed);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("header"));
    std::fs::remove_dir_all(dir).unwrap();
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

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("othello-book-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn trainer(args: &[String]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
        .args(args)
        .output()
        .unwrap()
}

fn sorted_lines(path: &Path) -> Vec<String> {
    let (_, records) = othello_tools::journal::read(path).unwrap();
    let mut lines: Vec<String> = records
        .iter()
        .map(|record| format!("{}\t{}", record.id, record.line))
        .collect();
    lines.sort();
    lines
}
