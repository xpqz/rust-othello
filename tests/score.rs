use othello::score::{EVAL_BOUND, INF, SCORE_SCALE, terminal_score};

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
fn terminal_score_range() {
    for diff in -64..=64 {
        assert_eq!(terminal_score(diff), diff * SCORE_SCALE);
    }
}

#[test]
fn infinity_negates() {
    assert_eq!(INF, 1_000_000);
    assert_eq!(-INF, -1_000_000);
    assert!(-INF > i32::MIN);
}

#[test]
fn eval_bound_fits() {
    assert_eq!(EVAL_BOUND, 6_400);
    assert!(EVAL_BOUND < INF);
    assert_eq!(terminal_score(64), EVAL_BOUND);
}

#[test]
fn window_arithmetic() {
    assert_eq!(-INF + EVAL_BOUND, -993_600);
    assert_eq!(INF - EVAL_BOUND, 993_600);
}
