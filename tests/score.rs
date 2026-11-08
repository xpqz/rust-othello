use othello::board::{Board, final_score};
use othello::score::{EVAL_BOUND, INF, SCORE_SCALE, terminal_score};

const _: () = assert!(EVAL_BOUND < INF);

#[test]
fn scale_is_centidiscs() {
    assert_eq!(SCORE_SCALE, 100);
    assert_eq!(terminal_score(0), 0);
    assert_eq!(terminal_score(1), 100);
    assert_eq!(terminal_score(-1), -100);
    assert_eq!(terminal_score(64), 6_400);
    assert_eq!(terminal_score(-64), -6_400);
}

#[test]
fn terminal_score_properties() {
    assert_eq!(EVAL_BOUND, 6_400);
    for diff in -64..=64 {
        let score = terminal_score(diff);
        assert_eq!(terminal_score(-diff), -score);
        assert!(score.abs() <= EVAL_BOUND);
        assert!(-INF < score && score < INF);
    }
    for diff in -64..64 {
        assert!(terminal_score(diff) < terminal_score(diff + 1));
    }
}

#[test]
fn infinity_negates() {
    assert_eq!(INF, 1_000_000);
    assert_eq!((-INF).checked_neg(), Some(INF));
    assert_eq!(INF.checked_neg(), Some(-INF));
}

#[test]
fn window_edges() {
    assert_eq!(INF.checked_add(1), Some(1_000_001));
    assert_eq!((-INF).checked_sub(1), Some(-1_000_001));
}

#[test]
fn score_types() {
    let _: (i32, i32, i32, fn(i32) -> i32) = (SCORE_SCALE, INF, EVAL_BOUND, terminal_score);
}

#[test]
fn terminal_wiring() {
    let board = Board {
        me: 0b11,
        opp: 1 << 63,
    };
    assert_eq!(terminal_score(final_score(board)), 6_200);

    let tied = Board {
        me: 1,
        opp: 1 << 63,
    };
    assert_eq!(terminal_score(final_score(tied)), 0);
}
