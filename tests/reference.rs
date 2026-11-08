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
