mod common;

use common::playout;
use othello::board::{Board, pass};
use othello::eval::evaluate;
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
        (at(3, 1), 99),
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
fn antisymmetry_and_bounds() {
    let mut rng = Rng::new(0xE7A1);
    for _ in 0..100 {
        playout(&mut rng, |board| {
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
