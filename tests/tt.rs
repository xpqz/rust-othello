use std::mem::size_of;

use othello::board::Board;
use othello::score::INF;
use othello::tt::{Bound, Domain, Entry, MAX_ENTRIES, NO_MOVE, Table, zobrist};

fn entry(key: u64, value: i32, depth: u8, flag: Bound, domain: Domain) -> Entry {
    Entry {
        key,
        value,
        depth,
        flag,
        domain,
        mv: NO_MOVE,
    }
}

#[test]
fn zobrist_keys() {
    let key = zobrist(Board::START);
    assert_ne!(key, 0);
    assert_eq!(key, zobrist(Board::START));
    let swapped = zobrist(Board {
        me: Board::START.opp,
        opp: Board::START.me,
    });
    assert_ne!(key, swapped);
    assert_eq!(key, 0xe63a_89a2_65d0_0287);
}

#[test]
fn entry_size() {
    assert_eq!(size_of::<Entry>(), 16);
}

#[test]
fn bound_cutoffs() {
    let exact = entry(1, 5, 3, Bound::Exact, Domain::Heuristic);
    assert_eq!(exact.cutoff(3, 0, 10), Some(5));

    let lower = entry(2, 10, 3, Bound::Lower, Domain::Heuristic);
    assert_eq!(lower.cutoff(3, 0, 10), Some(10));
    let weaker = entry(3, 9, 3, Bound::Lower, Domain::Heuristic);
    assert_eq!(weaker.cutoff(3, 0, 10), None);

    let upper = entry(4, 0, 3, Bound::Upper, Domain::Heuristic);
    assert_eq!(upper.cutoff(3, 0, 10), Some(0));
    let higher = entry(5, 1, 3, Bound::Upper, Domain::Heuristic);
    assert_eq!(higher.cutoff(3, 0, 10), None);
}

#[test]
fn strict_depth_and_domain() {
    let deep = entry(1, 5, 4, Bound::Exact, Domain::Heuristic);
    assert_eq!(deep.cutoff(3, -INF, INF), None);

    let endgame = entry(2, 5, 0, Bound::Exact, Domain::Endgame);
    assert_eq!(endgame.cutoff(0, -INF, INF), None);
    assert_eq!(endgame.endgame_cutoff(-INF, INF), Some(5));

    let lower = entry(3, 10, 0, Bound::Lower, Domain::Endgame);
    assert_eq!(lower.endgame_cutoff(0, 10), Some(10));
    assert_eq!(
        entry(4, 9, 0, Bound::Lower, Domain::Endgame).endgame_cutoff(0, 10),
        None
    );
    let upper = entry(5, 0, 0, Bound::Upper, Domain::Endgame);
    assert_eq!(upper.endgame_cutoff(0, 10), Some(0));
    assert_eq!(
        entry(6, 1, 0, Bound::Upper, Domain::Endgame).endgame_cutoff(0, 10),
        None
    );

    let heuristic = entry(7, 5, 0, Bound::Exact, Domain::Heuristic);
    assert_eq!(heuristic.endgame_cutoff(-INF, INF), None);
}

#[test]
fn order_hints() {
    let mut deep = entry(1, 0, 4, Bound::Exact, Domain::Heuristic);
    deep.mv = 7;
    assert_eq!(deep.move_hint(3), Some(7));
    assert_eq!(deep.move_hint(5), Some(7));
    deep.mv = NO_MOVE;
    assert_eq!(deep.move_hint(3), None);
    let endgame = entry(2, 0, 0, Bound::Exact, Domain::Endgame);
    assert_eq!(endgame.move_hint(0), None);
}

#[test]
fn table_operations() {
    let mut table = Table::new(8);
    table.store(entry(3, 5, 2, Bound::Exact, Domain::Heuristic));
    assert_eq!(table.probe(3).map(|entry| entry.value), Some(5));
    table.clear();
    assert!(table.probe(3).is_none());
    table.store(entry(0, 5, 2, Bound::Exact, Domain::Heuristic));
    assert!(table.probe(0).is_none());
}

#[test]
fn index_collision() {
    let mut table = Table::new(8);
    table.store(entry(5, 1, 0, Bound::Exact, Domain::Heuristic));
    table.store(entry(13, 2, 0, Bound::Exact, Domain::Heuristic));
    assert!(table.probe(5).is_none());
    assert_eq!(table.probe(13).map(|entry| entry.value), Some(2));
}

#[test]
fn memory_cap_is_64_mb() {
    assert_eq!(MAX_ENTRIES * std::mem::size_of::<Entry>(), 64 << 20);
    assert_eq!(Table::new(MAX_ENTRIES + 1).capacity(), MAX_ENTRIES);
}
