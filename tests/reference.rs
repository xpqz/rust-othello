mod common;

use common::Naive;
use othello::board::{Board, play};

fn bit(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

fn sq(file: usize, rank: usize) -> u8 {
    (rank * 8 + file) as u8
}

fn opening() -> [u8; 4] {
    [sq(2, 3), sq(3, 2), sq(4, 5), sq(5, 4)]
}

#[test]
fn from_bits_roundtrip() {
    let boards = [
        (Board::START.me, Board::START.opp),
        (bit(0, 0) | bit(6, 2), bit(2, 2) | bit(7, 0)),
        (bit(1, 0) | bit(2, 1), bit(0, 0) | bit(3, 0)),
    ];
    for (me, opp) in boards {
        assert_eq!(Naive::from_bits(me, opp).bits(), (me, opp));
    }
}

#[test]
fn start_opening_moves() {
    let naive = Naive::start();
    assert_eq!(
        naive.legal_moves(),
        bit(2, 3) | bit(3, 2) | bit(4, 5) | bit(5, 4)
    );
    for square in opening() {
        assert_ne!(naive.flips(square), 0, "{square}");
    }
}

#[test]
fn c4_post_move() {
    let naive = Naive::start();
    let after = naive.play(sq(2, 3));
    assert_eq!(after.bits().0, bit(4, 4));
    assert_eq!(
        after.bits().1,
        bit(2, 3) | bit(3, 3) | bit(4, 3) | bit(3, 4)
    );
    assert_eq!(naive.flips(sq(2, 3)), bit(3, 3));
    assert_eq!(naive.bits(), Naive::start().bits());
}

#[test]
fn all_opening_replies() {
    let naive = Naive::start();
    for square in opening() {
        let (me, opp) = naive.play(square).bits();
        let board = play(Board::START, square);
        assert_eq!((me, opp), (board.me, board.opp), "{square}");
    }
}

#[test]
fn reference_legal_fixtures() {
    let cases = [
        (0, 0, 0),
        (
            bit(0, 0),
            bit(1, 0) | bit(2, 0) | bit(3, 0) | bit(4, 0) | bit(5, 0) | bit(6, 0),
            bit(7, 0),
        ),
        (bit(0, 0), bit(1, 1), bit(2, 2)),
        (bit(7, 0), bit(6, 1), bit(5, 2)),
        (bit(0, 7), bit(1, 6), bit(2, 5)),
        (bit(7, 7), bit(6, 6), bit(5, 5)),
        (bit(7, 0), bit(0, 1), 0),
        (bit(0, 0) | bit(2, 0), bit(1, 0), 0),
        (bit(0, 0), bit(1, 0) | bit(3, 0), bit(2, 0)),
    ];
    for (me, opp, expected) in cases {
        assert_eq!(
            Naive::from_bits(me, opp).legal_moves(),
            expected,
            "{me:#x}/{opp:#x}"
        );
    }
}

#[test]
fn reference_flip_fixtures() {
    let multi = Naive::from_bits(bit(0, 0) | bit(2, 2), bit(1, 0) | bit(2, 1) | bit(3, 0));
    assert_eq!(multi.flips(sq(2, 0)), bit(1, 0) | bit(2, 1));

    let wrap = Naive::from_bits(bit(7, 2) | bit(1, 1), bit(7, 1) | bit(0, 1));
    assert_eq!(wrap.flips(sq(7, 0)), bit(7, 1));

    let long = Naive::from_bits(
        bit(0, 0),
        bit(1, 0) | bit(2, 0) | bit(3, 0) | bit(4, 0) | bit(5, 0) | bit(6, 0),
    );
    assert_eq!(
        long.flips(sq(7, 0)),
        bit(1, 0) | bit(2, 0) | bit(3, 0) | bit(4, 0) | bit(5, 0) | bit(6, 0)
    );
}

#[test]
fn pass_swaps() {
    let (me, opp) = (bit(0, 0) | bit(2, 2), bit(1, 0) | bit(3, 3));
    assert_eq!(Naive::from_bits(me, opp).pass().bits(), (opp, me));
}
