mod common;

use common::playout;
use othello::board::{Board, is_terminal, legal_moves, pass, play};
use othello::eval::{evaluate, frontier, mobility};
use othello::rng::Rng;
use othello::score::EVAL_BOUND;

fn at(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

#[test]
fn start_is_zero() {
    assert_eq!(evaluate(Board::START), 0);
}

#[test]
fn single_disc_values() {
    let cases = [
        (at(0, 0), 200),
        (at(1, 0), 80),
        (at(1, 1), 50),
        (at(2, 0), 110),
        (at(3, 0), 105),
        (at(2, 1), 98),
        (at(3, 1), 98),
        (at(2, 2), 99),
    ];
    for (me, expected) in cases {
        assert_eq!(evaluate(Board { me, opp: 0 }), expected, "{me:#x}");
    }
}

#[test]
fn mixed_table_sum() {
    let me = at(0, 0) | at(7, 7);
    let opp = at(1, 0) | at(6, 7);
    assert_eq!(evaluate(Board { me, opp }), 240);
}

#[test]
fn signature() {
    let _: fn(Board) -> i32 = evaluate;
}

#[test]
fn antisymmetry_and_bounds() {
    let mut rng = Rng::new(0xE7A1);
    for _ in 0..100 {
        playout(&mut rng, |board| {
            if is_terminal(board) {
                return;
            }
            let value = evaluate(board);
            assert_eq!(value, -evaluate(pass(board)));
            assert!((-EVAL_BOUND..=EVAL_BOUND).contains(&value));
        });
    }
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "out of bounds")]
fn overflow_asserts() {
    let _ = evaluate(Board {
        me: u64::MAX,
        opp: 0,
    });
}

#[test]
#[cfg(not(debug_assertions))]
fn overflow_clamps() {
    assert_eq!(
        evaluate(Board {
            me: u64::MAX,
            opp: 0,
        }),
        EVAL_BOUND
    );
}

#[test]
fn mobility_unequal_counts() {
    let board = Board {
        me: 0x020c_fc00_4828_1800,
        opp: 0x0000_021a_1616_263d,
    };
    let mine = legal_moves(board.me, board.opp).count_ones() as i32;
    let theirs = legal_moves(board.opp, board.me).count_ones() as i32;
    assert_eq!((mine, theirs), (10, 11));
    let value = mobility(board);
    assert_ne!(value, 0);
    assert_eq!(value.signum(), (mine - theirs).signum());
}

#[test]
fn mobility_values() {
    let board = Board {
        me: 0x020c_fc00_4828_1800,
        opp: 0x0000_021a_1616_263d,
    };
    assert_eq!(mobility(board), -45);
    assert_eq!(mobility(pass(board)), 45);
}

#[test]
fn frontier_swap() {
    assert_eq!(frontier(Board::START), 0);
    let board = play(Board::START, 19);
    assert_eq!(frontier(board), 3);
    assert_eq!(frontier(pass(board)), -3);
    let mut rng = Rng::new(0xF20A);
    let mut checked = 0;
    for _ in 0..20 {
        playout(&mut rng, |board| {
            if is_terminal(board) {
                return;
            }
            assert_eq!(frontier(board), -frontier(pass(board)));
            checked += 1;
        });
    }
    assert!(checked > 0);
}
