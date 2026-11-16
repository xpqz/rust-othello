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
use othello::search::{ProbCut, SearchConfig, SearchLimits, search_tt};
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

/// Independent replay: the board after each number of placements, with a
/// forced pass applied straight after the placement that causes it.
fn replay_oracle(moves: &[u8]) -> Vec<Board> {
    let mut board = Board::START;
    let mut boards = vec![board];
    for &square in moves {
        board = play(board, square);
        if legal_moves(board.me, board.opp) == 0 && legal_moves(board.opp, board.me) != 0 {
            board = Board {
                me: board.opp,
                opp: board.me,
            };
        }
        boards.push(board);
    }
    boards
}

/// A legal game that continues past a forced pass, and the number of
/// placements after which the pass falls.
fn pass_game() -> (Vec<u8>, usize) {
    for seed in 1..20_000 {
        let mut rng = Rng::new(seed);
        let (mut board, mut moves, mut at) = (Board::START, Vec::new(), None);
        while !is_terminal(board) {
            if legal_moves(board.me, board.opp) == 0 {
                at.get_or_insert(moves.len());
                board = pass(board);
            }
            let legal: Vec<u8> = squares(legal_moves(board.me, board.opp)).collect();
            let square = legal[(rng.next_u64() % legal.len() as u64) as usize];
            moves.push(square);
            board = play(board, square);
            if let Some(k) = at
                && moves.len() > k + 2
            {
                return (moves, k);
            }
        }
    }
    panic!("no game with a pass");
}

/// A complete random game, ending in a terminal position.
fn full_game(seed: u64) -> Vec<u8> {
    let (mut rng, mut board, mut moves) = (Rng::new(seed), Board::START, Vec::new());
    while !is_terminal(board) {
        if legal_moves(board.me, board.opp) == 0 {
            board = pass(board);
        }
        let legal: Vec<u8> = squares(legal_moves(board.me, board.opp)).collect();
        let square = legal[(rng.next_u64() % legal.len() as u64) as usize];
        moves.push(square);
        board = play(board, square);
    }
    moves
}

#[test]
fn count_plies_are_placements() {
    // Limit P counts the positions after 0..P-1 placements.
    let list = games(1, 1, 0, 10);
    let oracle = replay_oracle(&list[0]);
    let counts = book::count(&list, 3);
    let expected: BTreeSet<Key> = oracle[..3].iter().map(|&b| canonical(b)).collect();
    assert_eq!(counts.keys().copied().collect::<BTreeSet<_>>(), expected);
}

#[test]
fn count_through_a_pass() {
    let (moves, k) = pass_game();
    let oracle = replay_oracle(&moves);
    let passed = canonical(oracle[k]);
    let unpassed = canonical(Board {
        me: oracle[k].opp,
        opp: oracle[k].me,
    });
    assert_eq!(
        legal_moves(oracle[k].opp, oracle[k].me),
        0,
        "fixture: a pass after placement {k}"
    );
    let list = vec![moves.clone()];
    // The cutoff straddles the pass: limit k excludes it, k + 1 includes it.
    assert!(!book::count(&list, k as u32).contains_key(&passed));
    let counts = book::count(&list, k as u32 + 1);
    assert!(counts.contains_key(&passed) && !counts.contains_key(&unpassed));
    assert!(counts.contains_key(&canonical(oracle[k - 1])));
    // The pass is not a ply: the next placement enters at limit k + 2.
    assert!(!counts.contains_key(&canonical(oracle[k + 1])));
    assert!(book::count(&list, k as u32 + 2).contains_key(&canonical(oracle[k + 1])));
    // An unfinished prefix counts its final recorded board.
    let prefix = vec![moves[..k].to_vec()];
    let counts = book::count(&prefix, 60);
    assert!(counts.contains_key(&passed) && !counts.contains_key(&unpassed));
    assert_eq!(counts.len(), k + 1);
}

#[test]
fn count_terminal_final() {
    let game = full_game(7);
    let last = *replay_oracle(&game).last().unwrap();
    assert!(is_terminal(last));
    assert!(book::count(&[game], 61).contains_key(&canonical(last)));
}

#[test]
fn normalize_contracts_passes() {
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

/// The stand-in's score, as `value` must combine it: an independent oracle.
fn stand_in_value(board: Board, depth: u32) -> i32 {
    let score =
        |b: Board| (b.me.count_ones() as i32 - b.opp.count_ones() as i32) * 100 + depth as i32;
    if is_terminal(board) {
        terminal_score(final_score(board))
    } else if legal_moves(board.me, board.opp) == 0 {
        -score(Board {
            me: board.opp,
            opp: board.me,
        })
    } else {
        score(board)
    }
}

/// Drop-out over a node whose children include `kind`, checked against the
/// oracle: sign, post-placement depth, chosen square and calls made.
fn dropout_fixture(kind: impl Fn(Board) -> bool) {
    let (board, _) = find(|board, square| kind(play(board, square)));
    let key = book::key(board);
    let node = book::board(key);
    let calls = RefCell::new(Vec::new());
    let (value, square) = book::dropout(key, &node_set([key]), 9, &mut recorder(&calls)).unwrap();
    let legal: Vec<u8> = squares(legal_moves(node.me, node.opp)).collect();
    let values: Vec<i32> = legal
        .iter()
        .map(|&s| -stand_in_value(play(node, s), 8))
        .collect();
    let best = *values.iter().max().unwrap();
    assert_eq!(
        (value, square),
        (best, legal[values.iter().position(|&v| v == best).unwrap()])
    );
    let expected: Vec<(Board, u32)> = legal
        .iter()
        .map(|&s| play(node, s))
        .filter(|&child| !is_terminal(child))
        .map(|child| {
            (
                if needs_pass(child) {
                    pass(child)
                } else {
                    child
                },
                8,
            )
        })
        .collect();
    assert_eq!(calls.into_inner(), expected);
    assert!(legal.iter().any(|&s| kind(play(node, s))));
}

#[test]
fn dropout_pass_child() {
    dropout_fixture(|child| needs_pass(child) && !is_terminal(pass(child)));
}

#[test]
fn dropout_terminal_child() {
    dropout_fixture(is_terminal);
}

/// Nodes: the start, its one canonical child, and every canonical child of
/// that, with hand-set drop-out results.
fn small_book() -> (BTreeMap<Key, u32>, book::Drops) {
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
fn shuffle_golden() {
    // Fisher–Yates with `Rng::new(150)`, j = next_u64() % (i + 1), i falling.
    let mut items: Vec<u32> = (0..10).collect();
    book::shuffle(&mut items, 150);
    assert_eq!(items, vec![0, 2, 9, 4, 3, 8, 1, 5, 6, 7]);
}

/// The registered shuffle, written out independently of the builder.
fn fisher_yates(n: usize, seed: u64) -> Vec<usize> {
    let (mut order, mut rng) = ((0..n).collect::<Vec<_>>(), Rng::new(seed));
    for i in (1..n).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    order
}

#[test]
fn roots_frozen() {
    let list = games(60, 6, 8, 12);
    let counts = book::count(&list, 12);
    let nodes: BTreeMap<Key, u32> = counts.into_iter().filter(|&(_, n)| n >= 2).collect();
    // The pool: movable node positions after 6 to 8 placements, sorted by key.
    let mut pool = BTreeSet::new();
    for game in &list {
        for (ply, &board) in replay_oracle(game).iter().enumerate() {
            let key = canonical(board);
            if (6..=8).contains(&ply)
                && nodes.contains_key(&key)
                && legal_moves(board.me, board.opp) != 0
            {
                pool.insert(key);
            }
        }
    }
    let pool: Vec<Key> = pool.into_iter().collect();
    let expected: Vec<Key> = fisher_yates(pool.len(), 151)
        .into_iter()
        .take(5)
        .map(|i| pool[i])
        .collect();
    let roots = book::roots(&list, &nodes, 6..=8, 5, 151).unwrap();
    let keys: Vec<Key> = roots
        .iter()
        .map(|line| canonical(*replay_oracle(line).last().unwrap()))
        .collect();
    assert_eq!(keys, expected);
    for line in &roots {
        assert!((6..=8).contains(&line.len()));
        let first = list.iter().find(|game| {
            game.len() >= line.len()
                && canonical(replay_oracle(&game[..line.len()])[line.len()])
                    == canonical(*replay_oracle(line).last().unwrap())
        });
        assert_eq!(Some(&line[..]), first.map(|game| &game[..line.len()]));
    }
    assert!(book::roots(&list, &nodes, 6..=8, pool.len() + 1, 151).is_err());
}

#[test]
fn projection_and_rule() {
    // 10,000 resamples of 4 draws (index next_u64() % 4, one Rng(150)), the
    // upper bound at sorted[ceil(0.975 * 10,000) - 1].
    let times = [1.0, 2.0, 3.0, 10.0];
    let (mean, upper) = book::projection(&times, 1000, 5, 150);
    assert_eq!(mean, 0.2222222222222222);
    assert_eq!(upper, 0.4444444444444444);
    assert_eq!(
        book::depth_rule(&[(18, 10.0), (19, 30.0), (20, 40.0)], 36.0),
        Some(19)
    );
    assert_eq!(
        book::depth_rule(&[(18, 10.0), (19, 36.0), (20, 36.1)], 36.0),
        Some(19)
    );
    assert_eq!(
        book::depth_rule(&[(18, 1.0), (19, 2.0), (20, 3.0)], 36.0),
        Some(20)
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

fn identity() -> String {
    book::identity(&TEST_PARAMS, "test")
}

const TEST_PARAMS: Params = Params {
    min: 2,
    plies: 8,
    depth: 2,
    table: 1 << 12,
};

/// Independent oracle for a child's value under the scoring policy, with a
/// fresh table per search.
fn policy_value(board: Board, depth: u32) -> i32 {
    if is_terminal(board) {
        terminal_score(final_score(board))
    } else if legal_moves(board.me, board.opp) == 0 {
        -policy_value(
            Board {
                me: board.opp,
                opp: board.me,
            },
            depth,
        )
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

/// Decoded asset records: (zobrist, square, value).
fn records(asset: &[u8]) -> Vec<(u64, u8, i16)> {
    asset
        .as_chunks::<11>()
        .0
        .iter()
        .map(|r| {
            let key = u64::from_le_bytes(r[..8].try_into().unwrap());
            (key, r[8], i16::from_le_bytes([r[9], r[10]]))
        })
        .collect()
}

fn manifest_line(key: Key, square: u8, value: i16) -> String {
    format!("{:016x}:{:016x} {square} {value}", key.0, key.1)
}

fn manifest_records(manifest: &str) -> Vec<&str> {
    manifest
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect()
}

#[test]
fn artifact_records() {
    // A node with a terminal child: the terminal node is in the input but
    // never emitted; the node's record matches an independent computation.
    let (board, _) = find(|board, square| is_terminal(play(board, square)));
    let parent = book::key(board);
    let node = book::board(parent);
    let legal: Vec<u8> = squares(legal_moves(node.me, node.opp)).collect();
    let end = legal
        .iter()
        .map(|&s| book::key(play(node, s)))
        .find(|&k| is_terminal(book::board(k)))
        .unwrap();
    let nodes = BTreeMap::from([(parent, 10), (end, 10)]);
    let dir = temp_dir("records");
    let journal = dir.join("records.journal");
    book::score(
        &[parent, end],
        &TEST_PARAMS,
        1,
        &journal,
        &book::header(&TEST_PARAMS, "test"),
    )
    .unwrap();
    let roots = [(parent, true), (parent, false)];
    let (asset, manifest) = book::build(
        &nodes,
        &journal,
        &identity(),
        &roots,
        &BTreeSet::from([parent]),
        1 << 16,
    )
    .unwrap();
    let values: Vec<i32> = legal
        .iter()
        .map(|&s| -policy_value(play(node, s), 1))
        .collect();
    let best = *values.iter().max().unwrap();
    let square = legal[values.iter().position(|&v| v == best).unwrap()];
    let value = best.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    assert_eq!(
        records(&asset),
        vec![(othello::tt::zobrist(node), square, value)]
    );
    assert_eq!(
        manifest_records(&manifest),
        vec![manifest_line(parent, square, value)]
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn artifact_truncation_keeps_parent() {
    // Truncating to the start record keeps exactly the start's full-book
    // record; its children are gone from asset and manifest.
    let one = book::key(play(Board::START, 19));
    let one_board = book::board(one);
    let mut keys = vec![START, one];
    keys.extend(squares(legal_moves(one_board.me, one_board.opp)).map(|s| child_key(one_board, s)));
    keys.sort();
    keys.dedup();
    let nodes: BTreeMap<Key, u32> = keys
        .iter()
        .map(|&k| (k, if k == START { 100 } else { 5 }))
        .collect();
    let dir = temp_dir("truncation");
    let journal = dir.join("truncation.journal");
    book::score(
        &keys,
        &TEST_PARAMS,
        2,
        &journal,
        &book::header(&TEST_PARAMS, "test"),
    )
    .unwrap();
    let roots = [(START, true)];
    let mandatory = BTreeSet::from([START]);
    let (full, full_manifest) =
        book::build(&nodes, &journal, &identity(), &roots, &mandatory, 1 << 16).unwrap();
    let (cut, cut_manifest) =
        book::build(&nodes, &journal, &identity(), &roots, &mandatory, 11).unwrap();
    let start = othello::tt::zobrist(Board::START);
    let kept: Vec<_> = records(&full)
        .into_iter()
        .filter(|r| r.0 == start)
        .collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].1, 19);
    assert!(records(&full).len() > 1);
    assert_eq!(records(&cut), kept);
    let line = manifest_line(START, kept[0].1, kept[0].2);
    assert!(manifest_records(&full_manifest).contains(&line.as_str()));
    assert_eq!(manifest_records(&cut_manifest), vec![line.as_str()]);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Scores every node of a small game set at depth 2 and builds the asset.
fn build_with(
    dir: &Path,
    name: &str,
    nodes: &[Key],
    jobs: usize,
) -> (Vec<String>, Vec<u8>, String) {
    let journal = dir.join(format!("{name}.journal"));
    book::score(
        nodes,
        &TEST_PARAMS,
        jobs,
        &journal,
        &book::header(&TEST_PARAMS, "test"),
    )
    .unwrap();
    let node_map: BTreeMap<Key, u32> = nodes.iter().map(|&k| (k, 10)).collect();
    let (asset, manifest) = book::build(
        &node_map,
        &journal,
        &identity(),
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
    assert!(!one.1.is_empty());
    let manifest: Vec<String> = records(&one.1)
        .iter()
        .map(|&(zobrist, square, value)| {
            let key = *nodes
                .iter()
                .find(|&&k| othello::tt::zobrist(book::board(k)) == zobrist)
                .unwrap();
            manifest_line(key, square, value)
        })
        .collect();
    assert_eq!(
        manifest_records(&one.2),
        manifest.iter().map(String::as_str).collect::<Vec<_>>()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

/// Kills its child on drop, so a failed assertion never leaves one running.
struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn score_args(dir: &str, journal: &Path, depth: u32, jobs: usize) -> Vec<String> {
    let depth = depth.to_string();
    let jobs = jobs.to_string();
    [
        "book",
        "score",
        dir,
        journal.to_str().unwrap(),
        "--min",
        "2",
        "--plies",
        "12",
        "--depth",
        &depth,
        "--jobs",
        &jobs,
    ]
    .map(String::from)
    .to_vec()
}

/// "done D scored S" from the score command's summary line.
fn progress(output: &std::process::Output) -> (usize, usize) {
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.contains(" done "))
        .expect("summary line");
    let field = |name: &str| -> usize {
        let at = line.split_whitespace().position(|w| w == name).unwrap();
        line.split_whitespace()
            .nth(at + 1)
            .unwrap()
            .parse()
            .unwrap()
    };
    (field("done"), field("scored"))
}

#[test]
fn score_resume() {
    let dir = temp_dir("resume");
    let list = games(30, 3, 10, 40);
    std::fs::write(dir.join("games.wtb"), archive(&list)).unwrap();
    let dir_arg = dir.to_str().unwrap();
    let killed = dir.join("killed.journal");
    let nodes: BTreeMap<Key, u32> = book::count(&list, 12)
        .into_iter()
        .filter(|&(_, n)| n >= 2)
        .collect();
    // Deepen until the kill lands while units remain, so it always
    // interrupts real work, in debug and release alike.
    let mut chosen = None;
    for depth in [6, 8, 10, 12] {
        let _ = std::fs::remove_file(&killed);
        let mut child = Running(
            Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
                .args(score_args(dir_arg, &killed, depth, 1))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let start = Instant::now();
        while othello_tools::journal::read(&killed).map_or(true, |(_, r)| r.is_empty()) {
            assert!(
                start.elapsed() < Duration::from_secs(120),
                "no journal line before timeout"
            );
            sleep(Duration::from_millis(1));
        }
        child.0.kill().unwrap();
        let signalled = child.0.wait().unwrap().signal() == Some(9);
        if signalled && sorted_lines(&killed).len() < nodes.len() {
            chosen = Some(depth);
            break;
        }
    }
    let depth = chosen.expect("every depth finished before the kill");
    let partial = std::fs::read(&killed).unwrap();
    let done = sorted_lines(&killed).len();

    let full = dir.join("full.journal");
    let output = trainer(&score_args(dir_arg, &full, depth, 1));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = sorted_lines(&full);
    assert_eq!(expected.len(), nodes.len());
    assert!(done < expected.len());

    // Resume: completed units are skipped, and the file only grows.
    let output = trainer(&score_args(dir_arg, &killed, depth, 3));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(progress(&output), (done, expected.len() - done));
    let resumed = std::fs::read(&killed).unwrap();
    let complete = partial.len() - partial.iter().rev().take_while(|&&b| b != b'\n').count();
    assert_eq!(&resumed[..complete], &partial[..complete]);
    assert_eq!(sorted_lines(&killed), expected);

    // A torn last record is dropped and redone.
    let torn = dir.join("torn.journal");
    let text = std::fs::read(&full).unwrap();
    let cut = text[..text.len() - 1]
        .iter()
        .rposition(|&b| b == b'\n')
        .unwrap()
        + 1;
    let mut bytes = text[..cut].to_vec();
    bytes.extend_from_slice(&text[cut..cut + (text.len() - cut) / 2]);
    std::fs::write(&torn, bytes).unwrap();
    let output = trainer(&score_args(dir_arg, &torn, depth, 2));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(progress(&output), (expected.len() - 1, 1));
    assert_eq!(sorted_lines(&torn), expected);

    // The resumed journals build exactly the full journal's asset.
    let roots = [(START, true), (START, false)];
    let mandatory = BTreeSet::from([START]);
    let header = othello_tools::journal::read(&full).unwrap().0;
    let identity = header.split(" depth ").next().unwrap().to_string();
    let built = |journal: &Path| {
        book::build(&nodes, journal, &identity, &roots, &mandatory, 1 << 16).unwrap()
    };
    assert_eq!(built(&killed), built(&full));
    assert_eq!(built(&torn), built(&full));

    // A changed input is refused on the header, and the journal is untouched.
    let before = std::fs::read(&full).unwrap();
    let output = trainer(&score_args(dir_arg, &full, depth + 1, 1));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("header"));
    assert_eq!(std::fs::read(&full).unwrap(), before);
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

#[test]
fn roots_file_round_trip() {
    let list = games(60, 6, 8, 12);
    let nodes: BTreeMap<Key, u32> = book::count(&list, 12)
        .into_iter()
        .filter(|&(_, n)| n >= 2)
        .collect();
    let roots = book::roots(&list, &nodes, 6..=8, 5, 151).unwrap();
    let text = book::write_lines(&roots, "test roots");
    assert!(text.starts_with("# test roots\n"));
    let keys: Vec<Key> = roots
        .iter()
        .map(|line| canonical(*replay_oracle(line).last().unwrap()))
        .collect();
    assert_eq!(book::read_roots(&text).unwrap(), keys);
    assert!(book::read_roots("f5 f5\n").is_err());
    assert!(book::read_roots("z9\n").is_err());
}

#[test]
fn count_rejects_illegal_game() {
    // An illegal second move leaves the whole game out; an unfinished legal
    // game still counts.
    assert!(!book::legal_game(&[19, 19]));
    assert!(book::count(&[vec![19, 19]], 10).is_empty());
    assert!(book::legal_game(&[19]));
    assert_eq!(book::count(&[vec![19]], 10).len(), 2);
}

/// A journal with the given header and records, written directly.
fn journal_with(dir: &Path, name: &str, header: &str, records: &[(Key, &str)]) -> PathBuf {
    let path = dir.join(name);
    let (mut journal, _) = othello_tools::journal::Journal::open(&path, header).unwrap();
    for &(k, line) in records {
        journal
            .append(&format!("{:016x}:{:016x}", k.0, k.1), line)
            .unwrap();
    }
    path
}

#[test]
fn build_validates_journal() {
    let dir = temp_dir("validate");
    let header = book::header(&TEST_PARAMS, "test");
    let one = book::key(play(Board::START, 19));
    let only = BTreeMap::from([(START, 10)]);
    let both = BTreeMap::from([(START, 10), (one, 10)]);
    let roots = [(START, true)];
    let mandatory = BTreeSet::from([START]);
    let build = |nodes: &BTreeMap<Key, u32>, path: &Path| {
        book::build(nodes, path, &identity(), &roots, &mandatory, 1 << 16)
    };
    // The reviewer's probe: a foreign header and an illegal square.
    let foreign = journal_with(&dir, "foreign", "unrelated header", &[(START, "123 0 | ")]);
    assert!(build(&only, &foreign).is_err());
    let illegal = journal_with(&dir, "illegal", &header, &[(START, "123 0 | ")]);
    assert!(build(&only, &illegal).is_err());
    let legal = journal_with(&dir, "legal", &header, &[(START, "123 19 | ")]);
    assert!(build(&only, &legal).is_ok());
    let repeated = journal_with(
        &dir,
        "repeated",
        &header,
        &[(START, "123 19 | "), (START, "123 19 | ")],
    );
    assert!(build(&only, &repeated).is_err());
    // A node set that differs from the journal's: missing, extra, and a
    // result whose classification no longer holds.
    assert!(build(&both, &legal).is_err());
    let scored = journal_with(
        &dir,
        "scored",
        &header,
        &[(START, "none | "), (one, "5 18 | ")],
    );
    assert!(build(&only, &scored).is_err());
    let one_only = journal_with(&dir, "start-none", &header, &[(START, "none | ")]);
    assert!(build(&only, &one_only).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn build_cli_writes_nothing_on_refusal() {
    let dir = temp_dir("build-cli");
    std::fs::write(dir.join("games.wtb"), archive(&games(30, 3, 10, 40))).unwrap();
    std::fs::write(dir.join("roots.txt"), "# roots\n").unwrap();
    let journal = journal_with(
        &dir,
        "foreign.journal",
        "unrelated header",
        &[(START, "123 0 | ")],
    );
    let (asset, manifest) = (dir.join("book.bin"), dir.join("manifest.txt"));
    let path = |p: &Path| p.to_str().unwrap().to_string();
    let output = trainer(&[
        "book".into(),
        "build".into(),
        path(&dir),
        path(&journal),
        "--min".into(),
        "2".into(),
        "--plies".into(),
        "12".into(),
        "--roots".into(),
        path(&dir.join("roots.txt")),
        "--asset".into(),
        path(&asset),
        "--manifest".into(),
        path(&manifest),
    ]);
    assert!(!output.status.success());
    assert!(!asset.exists() && !manifest.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_rejects_before_writing() {
    let dir = temp_dir("reject");
    std::fs::write(dir.join("games.wtb"), archive(&games(4, 2, 4, 8))).unwrap();
    let journal = dir.join("never.journal");
    let base = |command: &str| -> Vec<String> {
        [
            "book",
            command,
            dir.to_str().unwrap(),
            journal.to_str().unwrap(),
        ]
        .map(String::from)
        .to_vec()
    };
    let cases: [(&str, &[&str]); 12] = [
        ("score", &["--depth", "4294967297"]),
        ("score", &["--depth", "0"]),
        ("score", &["--jobs", "0"]),
        ("score", &["--min", "2", "--min", "3"]),
        ("score", &["--foo", "1"]),
        ("score", &["--count", "5"]),
        ("score", &["--depth"]),
        ("pilot", &["--depths", ""]),
        ("pilot", &["--depths", "18,18"]),
        ("pilot", &["--limit", "NaN"]),
        ("build", &["--roots", "r", "--asset", "a"]),
        ("roots", &["--seed", "-1"]),
    ];
    for (command, extra) in cases {
        let mut args = base(command);
        args.extend(extra.iter().map(|s| s.to_string()));
        let output = trainer(&args);
        assert!(!output.status.success(), "{command} {extra:?} accepted");
        assert!(!journal.exists(), "{command} {extra:?} wrote its path");
    }
    let empty = book::score(
        &[START],
        &Params {
            depth: 0,
            ..TEST_PARAMS
        },
        1,
        &journal,
        "h",
    );
    assert!(empty.is_err() && !journal.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn score_reports_progress() {
    let dir = temp_dir("progress");
    std::fs::write(dir.join("games.wtb"), archive(&games(30, 3, 10, 40))).unwrap();
    let journal = dir.join("progress.journal");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
            .args(score_args(dir.to_str().unwrap(), &journal, 4, 2))
            .env("OTHELLO_BOOK_PROGRESS_MS", "1")
            .output()
            .unwrap()
    };
    let first = String::from_utf8(run().stdout).unwrap();
    let total = first
        .lines()
        .find_map(|l| l.strip_prefix("progress units 0/"))
        .unwrap();
    let total: usize = total.split_whitespace().next().unwrap().parse().unwrap();
    let summary = first.lines().position(|l| l.contains(" done ")).unwrap();
    assert!(
        first
            .lines()
            .position(|l| l.starts_with("progress "))
            .unwrap()
            < summary
    );
    let again = String::from_utf8(run().stdout).unwrap();
    assert!(again.contains(&format!("progress units {total}/{total} ")));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pilot_decision_from_saved_records() {
    // Saved times decide the depth whatever their order in the journal or
    // the worker count of the resuming run; foreign or repeated records fail.
    let dir = temp_dir("pilot");
    std::fs::write(dir.join("games.wtb"), archive(&games(30, 3, 10, 40))).unwrap();
    let journal = dir.join("pilot.journal");
    let args = |path: &Path, jobs: &str| -> Vec<String> {
        let d = dir.to_str().unwrap();
        [
            "book",
            "pilot",
            d,
            path.to_str().unwrap(),
            "--min",
            "2",
            "--plies",
            "12",
            "--depths",
            "1,2",
            "--jobs",
            jobs,
        ]
        .map(String::from)
        .to_vec()
    };
    let output = trainer(&args(&journal, "1"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (header, records) = othello_tools::journal::read(&journal).unwrap();
    assert!(header.contains("pilot sample 200 seed 150 depths [1, 2] threads 5 limit 36"));
    let mut ids: Vec<String> = records.iter().map(|r| r.id.clone()).collect();
    ids.sort();
    let saved = |name: &str, order: &[String]| -> PathBuf {
        let path = dir.join(name);
        let mut text = format!("{header}\n");
        for id in order {
            let at = ids.iter().position(|i| i == id).unwrap();
            text.push_str(&format!("{id}\t{} None\n", 1.0 + at as f64));
        }
        std::fs::write(&path, text).unwrap();
        path
    };
    let decision = |path: &Path, jobs: &str| -> Vec<String> {
        let output = trainer(&args(path, jobs));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("depth ") || l.starts_with("rule "))
            .map(String::from)
            .collect()
    };
    let mut reversed = ids.clone();
    reversed.reverse();
    let forward = decision(&saved("forward", &ids), "1");
    assert_eq!(forward, decision(&saved("reversed", &reversed), "3"));
    // The bound is the projection of the times in sorted-ID order.
    let nodes = book::count(&games(30, 3, 10, 40), 12)
        .into_iter()
        .filter(|&(_, n)| n >= 2)
        .count();
    let ones: Vec<f64> = ids
        .iter()
        .enumerate()
        .filter(|(_, id)| id.ends_with(":1"))
        .map(|(at, _)| 1.0 + at as f64)
        .collect();
    let (mean, upper) = book::projection(&ones, nodes, 5, 150);
    assert!(
        forward[0].ends_with(&format!("mean {mean:.4} upper {upper:.4}")),
        "{forward:?}"
    );
    let mut repeated = ids.clone();
    repeated.push(ids[0].clone());
    assert!(
        !trainer(&args(&saved("repeated", &repeated), "1"))
            .status
            .success()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn build_audit() {
    // The manifest names every removal in order and the coverage per root.
    let one = book::key(play(Board::START, 19));
    let one_board = book::board(one);
    let mut keys = vec![START, one];
    keys.extend(squares(legal_moves(one_board.me, one_board.opp)).map(|s| child_key(one_board, s)));
    keys.sort();
    keys.dedup();
    let nodes: BTreeMap<Key, u32> = keys
        .iter()
        .map(|&k| (k, if k == START { 100 } else { 5 }))
        .collect();
    let dir = temp_dir("audit");
    let journal = dir.join("audit.journal");
    book::score(
        &keys,
        &TEST_PARAMS,
        1,
        &journal,
        &book::header(&TEST_PARAMS, "test"),
    )
    .unwrap();
    let roots = [(START, true)];
    let mandatory = BTreeSet::from([START]);
    let (_, manifest) = book::build(&nodes, &journal, &identity(), &roots, &mandatory, 11).unwrap();
    let mut expected: Vec<Key> = keys
        .iter()
        .copied()
        .filter(|&k| k != START && k != one)
        .collect();
    expected.sort_by_key(|&k| othello::tt::zobrist(book::board(k)));
    let removed: Vec<String> = expected
        .iter()
        .map(|k| format!("# removed {:016x}:{:016x} count 5", k.0, k.1))
        .collect();
    let found: Vec<&str> = manifest
        .lines()
        .filter(|l| l.starts_with("# removed "))
        .collect();
    assert_eq!(
        found,
        removed.iter().map(String::as_str).collect::<Vec<_>>()
    );
    let root = format!(
        "# root {:016x}:{:016x} first 1 second 0 replies 0/4",
        START.0, START.1
    );
    assert!(manifest.lines().any(|l| l == root), "{manifest}");
    assert!(
        manifest
            .lines()
            .next()
            .unwrap()
            .starts_with("# book v1 min 2 plies 8 input test depth 2")
    );
    std::fs::remove_dir_all(dir).unwrap();
}
