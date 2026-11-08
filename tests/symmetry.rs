mod common;

use common::{Sym, transform};
use othello::board::{Board, legal_moves, pass, play};
use othello::rng::Rng;

fn at(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

fn sample() -> u64 {
    at(0, 0) | at(1, 0) | at(3, 4) | at(6, 2) | at(0, 7) | at(7, 7)
}

fn apply(bits: u64, syms: &[Sym]) -> u64 {
    syms.iter().fold(bits, |bits, &sym| transform(bits, sym))
}

fn inverse(sym: Sym) -> Sym {
    match sym {
        Sym::Identity => Sym::Identity,
        Sym::Rot90 => Sym::Rot270,
        Sym::Rot180 => Sym::Rot180,
        Sym::Rot270 => Sym::Rot90,
        Sym::FlipH => Sym::FlipH,
        Sym::FlipV => Sym::FlipV,
        Sym::FlipDiag => Sym::FlipDiag,
        Sym::FlipAnti => Sym::FlipAnti,
    }
}

fn nth_bit(mut bits: u64, mut k: u32) -> u8 {
    while k > 0 {
        bits &= bits - 1;
        k -= 1;
    }
    bits.trailing_zeros() as u8
}

fn assert_commutes(board: Board) {
    let moves = legal_moves(board.me, board.opp);
    for sym in Sym::ALL {
        assert_eq!(
            transform(moves, sym),
            legal_moves(transform(board.me, sym), transform(board.opp, sym)),
            "{sym:?}"
        );
    }
}

#[test]
fn transform_orientation() {
    let b1 = at(1, 0);
    assert_eq!(Sym::ALL.len(), 8);
    assert_eq!(transform(b1, Sym::Identity), at(1, 0));
    assert_eq!(transform(b1, Sym::Rot90), at(7, 1));
    assert_eq!(transform(b1, Sym::Rot180), at(6, 7));
    assert_eq!(transform(b1, Sym::Rot270), at(0, 6));
    assert_eq!(transform(b1, Sym::FlipH), at(6, 0));
    assert_eq!(transform(b1, Sym::FlipV), at(1, 7));
    assert_eq!(transform(b1, Sym::FlipDiag), at(0, 1));
    assert_eq!(transform(b1, Sym::FlipAnti), at(7, 6));
}

#[test]
fn transform_extremes() {
    for sym in Sym::ALL {
        assert_eq!(transform(0, sym), 0, "{sym:?}");
        assert_eq!(transform(u64::MAX, sym), u64::MAX, "{sym:?}");
        assert_eq!(
            transform(sample(), sym).count_ones(),
            sample().count_ones(),
            "{sym:?}"
        );
    }
}

#[test]
fn identity_orders() {
    let bits = sample();
    for sym in Sym::ALL {
        assert_eq!(apply(bits, &[Sym::Identity, sym]), transform(bits, sym));
        assert_eq!(apply(bits, &[sym, Sym::Identity]), transform(bits, sym));
    }
    for sym in [Sym::Rot90, Sym::Rot270] {
        assert_ne!(apply(bits, &[sym, sym]), bits, "{sym:?} order is 4");
        assert_eq!(apply(bits, &[sym, sym, sym, sym]), bits, "{sym:?}");
    }
    for sym in [
        Sym::Rot180,
        Sym::FlipH,
        Sym::FlipV,
        Sym::FlipDiag,
        Sym::FlipAnti,
    ] {
        assert_ne!(transform(bits, sym), bits, "{sym:?} order is 2");
        assert_eq!(apply(bits, &[sym, sym]), bits, "{sym:?}");
    }
}

#[test]
fn compositions() {
    let bits = sample();
    let cases = [
        (Sym::Rot90, Sym::Rot90, Sym::Rot180),
        (Sym::Rot90, Sym::Rot270, Sym::Identity),
        (Sym::Rot270, Sym::Rot90, Sym::Identity),
        (Sym::Rot180, Sym::Rot90, Sym::Rot270),
        (Sym::FlipH, Sym::FlipV, Sym::Rot180),
        (Sym::FlipV, Sym::FlipH, Sym::Rot180),
        (Sym::FlipDiag, Sym::Rot90, Sym::FlipH),
        (Sym::Rot90, Sym::FlipDiag, Sym::FlipV),
        (Sym::FlipH, Sym::FlipAnti, Sym::Rot90),
        (Sym::FlipAnti, Sym::FlipH, Sym::Rot270),
    ];
    for (a, b, c) in cases {
        assert_eq!(apply(bits, &[a, b]), transform(bits, c), "{a:?} then {b:?}");
    }
}

#[test]
fn inverses() {
    let bits = sample();
    for sym in Sym::ALL {
        let inv = inverse(sym);
        assert_eq!(apply(bits, &[sym, inv]), bits, "{sym:?} then {inv:?}");
        assert_eq!(apply(bits, &[inv, sym]), bits, "{inv:?} then {sym:?}");
    }
}

#[test]
fn legal_moves_commute() {
    let mut rng = Rng::new(0x5EED);
    let mut positions = 0;
    let mut with_moves = 0;
    for _ in 0..200 {
        let mut board = Board::START;
        for _ in 0..60 {
            let moves = legal_moves(board.me, board.opp);
            assert_commutes(board);
            positions += 1;
            if moves == 0 {
                if legal_moves(board.opp, board.me) == 0 {
                    break;
                }
                board = pass(board);
                continue;
            }
            with_moves += 1;
            let k = (rng.next_u64() % moves.count_ones() as u64) as u32;
            board = play(board, nth_bit(moves, k));
        }
    }
    assert!(with_moves * 10 >= positions * 9, "{with_moves}/{positions}");
}
