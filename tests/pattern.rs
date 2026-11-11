mod common;

use std::collections::BTreeSet;

use common::{Sym, transform};
use othello::board::Board;
use othello::pattern;

const HAND_ME: u64 = 0x0209;
const HAND_OPP: u64 = 0x8000_0000_0800_4824;

fn hand() -> Board {
    Board {
        me: HAND_ME,
        opp: HAND_OPP,
    }
}

fn transformed(board: Board, sym: Sym) -> Board {
    Board {
        me: transform(board.me, sym),
        opp: transform(board.opp, sym),
    }
}

fn canonical(board: Board) -> Vec<u32> {
    let mut values: Vec<u32> = pattern::features(board)
        .into_iter()
        .map(pattern::canonical)
        .collect();
    values.sort();
    values
}

fn feature(board: Board, family: usize) -> u32 {
    let base = pattern::FAMILIES[family];
    let index = pattern::instances()
        .iter()
        .position(|instance| instance.family == family && instance.squares == base)
        .unwrap();
    pattern::features(board)[index]
}

#[test]
fn instances_match_symmetries() {
    let mut expected = Vec::new();
    let mut offset = 0u32;
    for (family, base) in pattern::FAMILIES.iter().enumerate() {
        let mut seen = BTreeSet::new();
        for sym in Sym::ALL {
            let squares: Vec<u8> = base
                .iter()
                .map(|&square| transform(1u64 << square, sym).trailing_zeros() as u8)
                .collect();
            let set: BTreeSet<u8> = squares.iter().copied().collect();
            if seen.insert(set) {
                expected.push((family, squares, offset));
            }
        }
        offset += 3u32.pow(base.len() as u32);
    }
    assert_eq!(pattern::INSTANCE_COUNT, 46);
    assert_eq!(pattern::TABLE_SLOTS, 167_265);
    assert_eq!(pattern::STAGES, 12);
    assert_eq!(pattern::instances().len(), expected.len());
    for (instance, (family, squares, offset)) in pattern::instances().iter().zip(expected) {
        assert_eq!(instance.family, family);
        assert_eq!(instance.squares, squares.as_slice());
        assert_eq!(instance.offset, offset);
    }
    let sets: BTreeSet<Vec<u8>> = pattern::instances()
        .iter()
        .map(|instance| {
            let mut set = instance.squares.to_vec();
            set.sort();
            set
        })
        .collect();
    assert_eq!(sets.len(), 46);
}

#[test]
fn index_convention() {
    let board = hand();
    assert_eq!(feature(board, 0), 46_459);
    assert_eq!(feature(board, 2), 13_897);
    assert_eq!(feature(board, 6), 4_432);
}

#[test]
fn index_ranges() {
    for board in [Board::START, hand()] {
        for (instance, &index) in pattern::instances().iter().zip(&pattern::features(board)) {
            let start = instance.offset;
            let end = start + 3u32.pow(instance.squares.len() as u32);
            assert!(index >= start && index < end, "{index} outside {start}..{end}");
        }
    }
}

#[test]
fn stage_boundaries() {
    for (empties, stage) in [
        (64, 0),
        (60, 0),
        (55, 1),
        (50, 2),
        (45, 3),
        (40, 4),
        (35, 5),
        (30, 6),
        (25, 7),
        (20, 8),
        (15, 9),
        (10, 10),
        (5, 11),
        (0, 11),
    ] {
        assert_eq!(pattern::stage(empties), stage, "{empties}");
    }
}

#[test]
fn canonical_symmetry() {
    let board = hand();
    let expected = canonical(board);
    for sym in Sym::ALL {
        assert_eq!(canonical(transformed(board, sym)), expected, "{sym:?}");
    }
}
