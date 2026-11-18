use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use othello::board::{Board, play};
use othello_tools::positions;
use othello_tools::sym::canonical;

fn hash(path: &Path) -> u64 {
    let mut value = 0xcbf2_9ce4_8422_2325u64;
    for byte in fs::read(path).unwrap() {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x0000_0100_0000_01b3);
    }
    value
}

fn end_keys(path: &Path) -> BTreeSet<(u64, u64)> {
    positions::load(path.to_str().unwrap())
        .unwrap()
        .into_iter()
        .map(|opening| {
            let mut board = Board::START;
            for square in opening.squares {
                board = play(board, square);
            }
            canonical(board)
        })
        .collect()
}

#[test]
fn fresh_set_is_disjoint_and_complete() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bench");
    let fresh = end_keys(&base.join("openings_probcut2.txt"));
    assert_eq!(fresh.len(), 1024);
    for file in [
        "openings_tune.txt",
        "openings_validation.txt",
        "openings_final.txt",
    ] {
        let old = end_keys(&base.join(file));
        assert!(fresh.is_disjoint(&old), "{file}");
    }
}

#[test]
fn frozen_sets_unchanged() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bench");
    assert_eq!(hash(&base.join("openings_tune.txt")), 0xa623_dd76_9759_8d6e);
    assert_eq!(
        hash(&base.join("openings_validation.txt")),
        0x488e_9921_0404_15bb
    );
    assert_eq!(
        hash(&base.join("openings_final.txt")),
        0xfe1d_fcaf_36ef_bdf2
    );
}

#[test]
fn fresh_set_is_spread_across_groups() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bench");
    let fresh = positions::load(base.join("openings_probcut2.txt").to_str().unwrap()).unwrap();
    let validation =
        positions::load(base.join("openings_validation.txt").to_str().unwrap()).unwrap();
    let count = |set: &[positions::Opening]| {
        set.iter()
            .map(|opening| opening.key)
            .collect::<BTreeSet<_>>()
            .len()
    };
    assert!(count(&fresh) >= count(&validation));
    let mut groups = std::collections::BTreeMap::new();
    for opening in &fresh {
        *groups.entry(opening.key).or_insert(0usize) += 1;
    }
    assert!(*groups.values().max().unwrap() <= 102);
    let mut first = std::collections::BTreeMap::new();
    for opening in fresh.iter().take(1000) {
        *first.entry(opening.key).or_insert(0usize) += 1;
    }
    assert!(*first.values().max().unwrap() <= 100);
}

#[test]
fn gate_disjoint() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bench");
    let gate = end_keys(&base.join("openings_probcut3.txt"));
    assert_eq!(gate.len(), 1024);
    for file in [
        "openings_tune.txt",
        "openings_validation.txt",
        "openings_final.txt",
        "openings_probcut2.txt",
    ] {
        assert!(gate.is_disjoint(&end_keys(&base.join(file))), "{file}");
    }
}

#[test]
fn gate_spread() {
    // The gate plays the first 400 openings: no group holds more than 10%.
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bench");
    let gate = positions::load(base.join("openings_probcut3.txt").to_str().unwrap()).unwrap();
    let mut groups = std::collections::BTreeMap::new();
    for opening in gate.iter().take(400) {
        *groups.entry(opening.key).or_insert(0usize) += 1;
    }
    assert!(*groups.values().max().unwrap() <= 40, "{groups:?}");
    assert!(groups.len() >= 20);
}

#[test]
fn time_disjoint() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bench");
    let time = end_keys(&base.join("openings_time.txt"));
    assert_eq!(time.len(), 1024);
    for file in [
        "openings_tune.txt",
        "openings_validation.txt",
        "openings_final.txt",
        "openings_probcut2.txt",
        "openings_probcut3.txt",
        "openings_book.txt",
    ] {
        assert!(time.is_disjoint(&end_keys(&base.join(file))), "{file}");
    }
}

#[test]
fn time_spread() {
    // The clock gate plays the first 400 openings: no group holds more than 10%.
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bench");
    let time = positions::load(base.join("openings_time.txt").to_str().unwrap()).unwrap();
    let mut groups = std::collections::BTreeMap::new();
    for opening in time.iter().take(400) {
        *groups.entry(opening.key).or_insert(0usize) += 1;
    }
    assert!(*groups.values().max().unwrap() <= 40, "{groups:?}");
    assert!(groups.len() >= 20);
}
