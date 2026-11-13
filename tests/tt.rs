mod common;

use std::collections::HashSet;
use std::mem::size_of;

use common::playout;
use othello::board::Board;
use othello::rng::Rng;
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
    assert_eq!(key, 0x33e1_ce12_151b_5cfc);
}

fn sample_keys() -> Vec<u64> {
    let mut rng = Rng::new(0x4B);
    let mut boards = HashSet::new();
    while boards.len() < 200_000 {
        playout(&mut rng, |board| {
            boards.insert((board.me, board.opp));
            boards.insert((board.opp, board.me));
        });
    }
    let mut keys: Vec<u64> = boards
        .iter()
        .map(|&(me, opp)| zobrist(Board { me, opp }))
        .collect();
    keys.sort_unstable();
    keys
}

/// Slots of the full-size table used by `keys`, and the most keys in one.
fn occupancy(keys: &[u64]) -> (usize, u32) {
    let mut slots = vec![0u32; MAX_ENTRIES];
    for &key in keys {
        slots[key as usize % MAX_ENTRIES] += 1;
    }
    let used = slots.iter().filter(|&&count| count != 0).count();
    (used, slots.into_iter().max().unwrap())
}

/// With n keys thrown evenly into m slots, about n - n^2 / 2m slots are used
/// (97.6% of n here), and the chance that some slot holds seven or more is
/// about m (n/m)^7 / 7!, under 1e-6.
fn spread(keys: &[u64]) -> bool {
    let (used, most) = occupancy(keys);
    used * 100 >= keys.len() * 97 && most <= 6
}

/// Keys of distinct positions differ and are non-zero, they spread over the
/// full-size table's slots, and both ends of the key are evenly filled.
#[test]
fn key_spread() {
    let keys = sample_keys();
    assert!(keys.len() >= 200_000 && keys[0] != 0);
    assert!(
        keys.windows(2).all(|pair| pair[0] != pair[1]),
        "repeated key"
    );
    assert!(spread(&keys), "{:?}", occupancy(&keys));
    let (mut low, mut high) = ([0u32; 1024], [0u32; 1024]);
    for &key in &keys {
        low[key as usize % 1024] += 1;
        high[(key >> 54) as usize] += 1;
    }
    let share = keys.len() as u32 / 1024;
    for count in low.into_iter().chain(high) {
        assert!(
            (share * 3 / 4..=share * 5 / 4).contains(&count),
            "{count} of {share}"
        );
    }
}

/// The slot criterion rejects keys whose index bits 10 to 21 are stuck,
/// which the two ten-bit histograms alone would pass.
#[test]
fn clustered_keys_rejected() {
    let stuck: Vec<u64> = sample_keys()
        .into_iter()
        .map(|key| key & !0x3f_fc00)
        .collect();
    assert!(!spread(&stuck), "{:?}", occupancy(&stuck));
}

/// 256 keys in one slot must be seen as 256, not wrapped away.
#[test]
fn crowded_slot_rejected() {
    let crowded = (1..=256u64).map(|high| high << 22);
    let apart = 1..=199_744u64;
    let keys: Vec<u64> = crowded.chain(apart).collect();
    assert_eq!(occupancy(&keys), (199_745, 256));
    assert!(!spread(&keys));
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
    assert_eq!(deep.move_hint(), Some(7));
    assert_eq!(deep.cutoff(5, -INF, INF), None);
    deep.mv = NO_MOVE;
    assert_eq!(deep.move_hint(), None);
    let mut endgame = entry(2, 0, 0, Bound::Exact, Domain::Endgame);
    endgame.mv = 7;
    assert_eq!(endgame.move_hint(), None);
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
