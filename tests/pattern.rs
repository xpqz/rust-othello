mod common;

use std::collections::BTreeSet;

use common::{Sym, transform};
use othello::board::{Board, empty_count};
use othello::pattern;
use othello::score::EVAL_BOUND;

const HAND_ME: u64 = 0x0209;
const HAND_OPP: u64 = 0x8000_0000_0800_4824;

fn hand() -> Board {
    Board {
        me: HAND_ME,
        opp: HAND_OPP,
    }
}

fn corpus() -> Vec<Board> {
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/bench/corpus.txt")).unwrap();
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let me = u64::from_str_radix(parts.next()?, 16).ok()?;
            let opp = u64::from_str_radix(parts.next()?, 16).ok()?;
            Some(Board { me, opp })
        })
        .filter(|board| (board.me | board.opp).count_ones() >= 40)
        .take(3)
        .collect()
}

fn boards() -> Vec<Board> {
    let mut boards = vec![Board::START];
    boards.extend(corpus());
    assert_eq!(boards.len(), 4);
    boards
}

fn feature(board: Board, family: usize) -> u32 {
    let base = pattern::FAMILIES[family];
    let (position, instance) = pattern::instances()
        .iter()
        .enumerate()
        .find(|(_, instance)| instance.family == family && instance.squares.as_slice() == base)
        .unwrap();
    pattern::features(board)[position] - instance.offset
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
        assert_eq!(instance.squares.as_slice(), squares.as_slice());
        assert_eq!(instance.offset, offset);
    }
    let sets: BTreeSet<Vec<u8>> = pattern::instances()
        .iter()
        .map(|instance| {
            let mut set = instance.squares.clone();
            set.sort();
            set
        })
        .collect();
    assert_eq!(sets.len(), 46);
}

#[test]
fn instances_closed() {
    let sets: Vec<(usize, BTreeSet<u8>)> = pattern::instances()
        .iter()
        .map(|instance| (instance.family, instance.squares.iter().copied().collect()))
        .collect();
    for instance in pattern::instances() {
        for sym in Sym::ALL {
            let transformed: BTreeSet<u8> = instance
                .squares
                .iter()
                .map(|&square| transform(1u64 << square, sym).trailing_zeros() as u8)
                .collect();
            assert!(
                sets.iter()
                    .any(|(family, set)| *family == instance.family && *set == transformed),
                "{:?} {sym:?}",
                instance.squares
            );
        }
    }
}

#[test]
fn index_convention() {
    let board = hand();
    assert_eq!(feature(board, 0), 46_459);
    assert_eq!(feature(board, 2), 13_897);
    assert_eq!(feature(board, 6), 4_432);
}

#[test]
fn features_reference() {
    for board in boards() {
        let features = pattern::features(board);
        for (instance, &index) in pattern::instances().iter().zip(&features) {
            let mut local = 0u32;
            let mut power = 1u32;
            for &square in &instance.squares {
                let digit = if board.me & (1u64 << square) != 0 {
                    1
                } else if board.opp & (1u64 << square) != 0 {
                    2
                } else {
                    0
                };
                local += digit * power;
                power *= 3;
            }
            assert_eq!(
                index,
                instance.offset + local,
                "{board:?} {:?}",
                instance.squares
            );
        }
    }
}

#[test]
fn index_ranges() {
    for board in boards() {
        for (instance, &index) in pattern::instances().iter().zip(&pattern::features(board)) {
            let start = instance.offset;
            let end = start + 3u32.pow(instance.squares.len() as u32);
            assert!(
                index >= start && index < end,
                "{index} outside {start}..{end}"
            );
        }
    }
}

#[test]
fn stage_boundaries() {
    for (empties, stage) in [
        (64, 0),
        (60, 0),
        (59, 0),
        (56, 0),
        (55, 1),
        (51, 1),
        (50, 2),
        (45, 3),
        (40, 4),
        (35, 5),
        (30, 6),
        (25, 7),
        (20, 8),
        (15, 9),
        (10, 10),
        (6, 10),
        (5, 11),
        (4, 11),
        (1, 11),
        (0, 11),
    ] {
        assert_eq!(pattern::stage(empties), stage, "{empties}");
    }
}

fn layout() -> Vec<(Board, i32)> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/eval.layout"))
        .unwrap();
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            if parts.next()? != "board" {
                return None;
            }
            let me = u64::from_str_radix(parts.next()?, 16).ok()?;
            let opp = u64::from_str_radix(parts.next()?, 16).ok()?;
            let value = parts.next()?.parse().ok()?;
            Some((Board { me, opp }, value))
        })
        .collect()
}

fn asset() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/eval.bin")).unwrap()
}

fn reference(board: Board, asset: &[u8]) -> i32 {
    let stage = pattern::stage(empty_count(board));
    let value: i32 = pattern::features(board)
        .iter()
        .map(|&feature| {
            let offset = (stage * pattern::TABLE_SLOTS + feature as usize) * 2;
            i16::from_le_bytes([asset[offset], asset[offset + 1]]) as i32
        })
        .sum();
    value.clamp(-EVAL_BOUND, EVAL_BOUND)
}

#[test]
fn evaluate_layout() {
    for (board, expected) in layout() {
        assert_eq!(pattern::evaluate(board), expected, "{board:?}");
    }
}

#[test]
fn evaluate_bound() {
    for board in boards() {
        assert!(pattern::evaluate(board).abs() <= EVAL_BOUND, "{board:?}");
    }
}

#[test]
fn evaluate_reference() {
    let asset = asset();
    for board in boards() {
        assert_eq!(
            pattern::evaluate(board),
            reference(board, &asset),
            "{board:?}"
        );
    }
}

#[test]
fn asset_length() {
    let length = std::fs::metadata(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/eval.bin"))
        .unwrap()
        .len();
    assert_eq!(length, 4_014_360);
}

#[test]
fn engine_source() {
    let source = include_str!("../src/pattern.rs");
    assert!(!source.contains("canonical"));
    assert!(!source.contains("othello_tools"));
}
