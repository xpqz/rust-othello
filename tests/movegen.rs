mod common;

use common::{Naive, playout};
use othello::board::{
    Board, Dir, disc_count, empty_count, final_score, flips, is_terminal, legal_moves, pass, play,
    shift,
};
use othello::rng::Rng;

fn bit(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

fn sq(file: usize, rank: usize) -> u8 {
    (rank * 8 + file) as u8
}

#[test]
fn shift_edges() {
    assert_eq!(shift(bit(0, 0), Dir::E), bit(1, 0));
    assert_eq!(shift(bit(7, 0), Dir::E), 0);
    assert_eq!(shift(bit(1, 0), Dir::W), bit(0, 0));
    assert_eq!(shift(bit(0, 0), Dir::W), 0);
    assert_eq!(shift(bit(0, 0), Dir::N), bit(0, 1));
    assert_eq!(shift(bit(0, 7), Dir::N), 0);
    assert_eq!(shift(bit(0, 1), Dir::S), bit(0, 0));
    assert_eq!(shift(bit(0, 0), Dir::S), 0);
    assert_eq!(shift(bit(0, 0), Dir::NE), bit(1, 1));
    assert_eq!(shift(bit(7, 0), Dir::NE), 0);
    assert_eq!(shift(bit(0, 0), Dir::NW), 0);
    assert_eq!(shift(bit(1, 1), Dir::SW), bit(0, 0));
    assert_eq!(shift(bit(0, 6), Dir::SE), bit(1, 5));
    assert_eq!(shift(bit(7, 7), Dir::SE), 0);
    assert_eq!(shift(bit(1, 5), Dir::NW), bit(0, 6));
    assert_eq!(shift(bit(0, 7), Dir::SW), 0);
}

#[test]
fn shift_masks() {
    assert_eq!(shift(u64::MAX, Dir::E).count_ones(), 56);
    assert_eq!(shift(u64::MAX, Dir::W).count_ones(), 56);
    assert_eq!(shift(u64::MAX, Dir::N).count_ones(), 56);
    assert_eq!(shift(u64::MAX, Dir::S).count_ones(), 56);
    assert_eq!(shift(u64::MAX, Dir::NE).count_ones(), 49);
    assert_eq!(shift(u64::MAX, Dir::NW).count_ones(), 49);
    assert_eq!(shift(u64::MAX, Dir::SE).count_ones(), 49);
    assert_eq!(shift(u64::MAX, Dir::SW).count_ones(), 49);
}

#[test]
fn start_board() {
    let start = Board::START;
    assert_eq!(
        start,
        Board {
            me: bit(3, 4) | bit(4, 3),
            opp: bit(3, 3) | bit(4, 4),
        }
    );
    let copy = start;
    assert_eq!(start, copy);
}

#[test]
fn start_moves() {
    let start = Board::START;
    let expected = bit(2, 3) | bit(3, 2) | bit(4, 5) | bit(5, 4);
    assert_eq!(legal_moves(start.me, start.opp), expected);
}

#[test]
fn legal_fixtures() {
    assert_eq!(legal_moves(0, 0), 0);

    let six_run = bit(1, 0) | bit(2, 0) | bit(3, 0) | bit(4, 0) | bit(5, 0) | bit(6, 0);
    assert_eq!(legal_moves(bit(0, 0), six_run), bit(7, 0));

    assert_eq!(legal_moves(bit(0, 0), bit(1, 1)), bit(2, 2));
    assert_eq!(legal_moves(bit(7, 0), bit(6, 1)), bit(5, 2));
    assert_eq!(legal_moves(bit(0, 7), bit(1, 6)), bit(2, 5));
    assert_eq!(legal_moves(bit(7, 7), bit(6, 6)), bit(5, 5));

    assert_eq!(legal_moves(bit(7, 0), bit(0, 1)), 0);
    assert_eq!(legal_moves(bit(0, 0) | bit(2, 0), bit(1, 0)), 0);
    assert_eq!(legal_moves(bit(0, 0), bit(1, 0) | bit(3, 0)), bit(2, 0));
}

#[test]
fn start_flips() {
    let start = Board::START;
    assert_eq!(flips(start, sq(2, 3)), bit(3, 3));
    assert_eq!(flips(start, sq(3, 2)), bit(3, 3));
    assert_eq!(flips(start, sq(4, 5)), bit(4, 4));
    assert_eq!(flips(start, sq(5, 4)), bit(4, 4));
}

#[test]
fn play_start() {
    let after = play(Board::START, sq(2, 3));
    assert_eq!(after.me, bit(4, 4));
    assert_eq!(after.opp, bit(2, 3) | bit(3, 3) | bit(4, 3) | bit(3, 4));
}

#[test]
fn counts_start() {
    let start = Board::START;
    assert_eq!(disc_count(start.me), 2);
    assert_eq!(disc_count(start.opp), 2);
    assert_eq!(empty_count(start), 60);

    let after = play(start, sq(2, 3));
    assert_eq!(disc_count(after.me), 1);
    assert_eq!(disc_count(after.opp), 4);
    assert_eq!(empty_count(after), 59);
}

#[test]
fn pass_swaps() {
    let passed = pass(Board::START);
    assert_eq!(passed.me, Board::START.opp);
    assert_eq!(passed.opp, Board::START.me);
}

#[test]
fn terminal_board() {
    let tied = Board {
        me: bit(0, 0),
        opp: bit(7, 7),
    };
    assert!(is_terminal(tied));
    assert_eq!(final_score(tied), 0);

    let won = Board {
        me: bit(0, 0) | bit(1, 0),
        opp: bit(7, 7),
    };
    assert!(is_terminal(won));
    assert_eq!(final_score(won), 62);
    assert_eq!(final_score(pass(won)), -62);

    let full = Board {
        me: (1 << 33) - 1,
        opp: !((1 << 33) - 1),
    };
    assert!(is_terminal(full));
    assert_eq!(final_score(full), 2);

    assert!(!is_terminal(Board::START));
    assert!(!is_terminal(Board {
        me: bit(1, 0),
        opp: bit(0, 0),
    }));
}

#[test]
fn flips_fixtures() {
    let multi = Board {
        me: bit(0, 0) | bit(2, 2),
        opp: bit(1, 0) | bit(2, 1) | bit(3, 0),
    };
    assert_eq!(flips(multi, sq(2, 0)), bit(1, 0) | bit(2, 1));

    let wrap = Board {
        me: bit(7, 2) | bit(1, 1),
        opp: bit(7, 1) | bit(0, 1),
    };
    assert_eq!(flips(wrap, sq(7, 0)), bit(7, 1));

    let long = Board {
        me: bit(0, 0),
        opp: bit(1, 0) | bit(2, 0) | bit(3, 0) | bit(4, 0) | bit(5, 0) | bit(6, 0),
    };
    assert_eq!(
        flips(long, sq(7, 0)),
        bit(1, 0) | bit(2, 0) | bit(3, 0) | bit(4, 0) | bit(5, 0) | bit(6, 0)
    );
}

#[test]
fn opening_invariants() {
    let start = Board::START;
    let before = disc_count(start.me) + disc_count(start.opp);
    for square in [sq(2, 3), sq(3, 2), sq(4, 5), sq(5, 4)] {
        assert_ne!(flips(start, square), 0);
        let after = play(start, square);
        assert_eq!(disc_count(after.me) + disc_count(after.opp), before + 1);
    }
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "illegal move")]
fn play_illegal() {
    let _ = play(Board::START, sq(0, 0));
}

fn differential(count: usize) {
    let mut rng = Rng::new(0x0D1FF);
    let mut seen = 0;
    while seen < count {
        playout(&mut rng, |board| {
            if seen >= count {
                return;
            }
            let naive = Naive::from_bits(board.me, board.opp);
            let moves = legal_moves(board.me, board.opp);
            assert_eq!(naive.legal_moves(), moves, "{seen}: {board:?}");
            let mut rest = moves;
            while rest != 0 {
                let square = rest.trailing_zeros() as u8;
                rest &= rest - 1;
                let after = play(board, square);
                assert_eq!(
                    (after.me, after.opp),
                    naive.play(square).bits(),
                    "{seen}: {board:?} {square}"
                );
            }
            seen += 1;
        });
    }
    assert_eq!(seen, count);
}

#[test]
fn differential_10k() {
    differential(10_000);
}

#[test]
#[ignore]
fn differential_200k() {
    differential(200_000);
}
