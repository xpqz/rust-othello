use othello::probcut::{ProbCut, SigmaModel, shallow_depth};
use othello::score::{EVAL_BOUND, INF};

fn linear() -> SigmaModel {
    SigmaModel::Linear {
        s0: 100,
        gap: 0,
        empty: 0,
    }
}

fn params() -> ProbCut {
    ProbCut::new(150, 900, 30, linear(), (3, 12), (18, 52)).unwrap()
}

#[test]
fn constructor_rejects_out_of_range_fields() {
    let sigma = linear();
    for (t, a) in [(49, 900), (401, 900), (150, 499), (150, 1501)] {
        assert!(ProbCut::new(t, a, 0, sigma, (3, 12), (18, 52)).is_err());
    }
    assert!(ProbCut::new(150, 900, 0, sigma, (5, 4), (18, 52)).is_err());
    assert!(ProbCut::new(150, 900, 0, sigma, (2, 12), (18, 52)).is_err());
    assert!(ProbCut::new(150, 900, 0, sigma, (3, 12), (52, 18)).is_err());
}

#[test]
fn constructor_rejects_nonpositive_sigma() {
    let linear = SigmaModel::Linear {
        s0: 0,
        gap: 0,
        empty: 0,
    };
    let edax = SigmaModel::EdaxStyle {
        a: 0,
        b: 0,
        c: 0,
        q2: 0,
        q1: 0,
        q0: 0,
    };
    let table = SigmaModel::StageTable {
        buckets: [(0, 0); 6],
    };
    for sigma in [linear, edax, table] {
        assert!(ProbCut::new(150, 900, 0, sigma, (3, 12), (18, 52)).is_err());
    }
}

#[test]
fn shallow_depth_table() {
    let expected = [1, 2, 3, 2, 3, 4, 5, 4, 5, 6];
    for (depth, shallow) in (3..=12).zip(expected) {
        assert_eq!(shallow_depth(depth), shallow);
        assert_eq!((depth - shallow) % 2, 0);
    }
}

#[test]
fn support_boundaries() {
    let pc = params();
    for (depth, empties) in [(3, 18), (3, 52), (12, 18), (12, 52)] {
        assert!(pc.supports(depth, empties));
    }
    for (depth, empties) in [(2, 18), (13, 18), (3, 17), (3, 53)] {
        assert!(!pc.supports(depth, empties));
    }
}

#[test]
fn rounding_is_pinned() {
    let pc = params();
    assert_eq!(pc.high(500, 8, 30), Some(689));
    assert_eq!(pc.high(-100, 8, 30), Some(23));
    assert_eq!(pc.low(-400, 8, 30), Some(-645));
    assert_eq!(pc.low(100, 8, 30), Some(-89));
}

#[test]
fn guards_skip_beyond_the_eval_bound() {
    let pc = params();
    assert_eq!(pc.high(EVAL_BOUND, 8, 30), None);
    assert_eq!(pc.high(INF, 8, 30), None);
    assert_eq!(pc.low(-EVAL_BOUND, 8, 30), None);
    assert_eq!(pc.low(-INF, 8, 30), None);
}

#[test]
fn sigma_linear() {
    let pc = ProbCut::new(
        150,
        900,
        30,
        SigmaModel::Linear {
            s0: 100,
            gap: 10,
            empty: 2,
        },
        (3, 12),
        (18, 52),
    )
    .unwrap();
    assert_eq!(pc.sigma(8, 30), 200);
    assert!(pc.sigma(3, 18) >= 1);
}

#[test]
fn sigma_edax_style() {
    let pc = ProbCut::new(
        150,
        900,
        30,
        SigmaModel::EdaxStyle {
            a: 1,
            b: 2,
            c: 3,
            q2: 1,
            q1: 2,
            q0: 100,
        },
        (3, 12),
        (18, 52),
    )
    .unwrap();
    assert_eq!(pc.sigma(8, 30), 3580);
    assert!(pc.sigma(3, 18) >= 1);
}

#[test]
fn sigma_stage_table() {
    let mut buckets = [(100, 10); 6];
    buckets[5] = (200, 20);
    let pc = ProbCut::new(
        150,
        900,
        30,
        SigmaModel::StageTable { buckets },
        (3, 12),
        (18, 52),
    )
    .unwrap();
    assert_eq!(pc.sigma(8, 18), 140);
    assert_eq!(pc.sigma(8, 48), 280);
    assert!(pc.sigma(3, 52) >= 1);
}

#[test]
fn extreme_values_do_not_panic() {
    let pc = ProbCut::new(
        400,
        1500,
        i32::MIN,
        SigmaModel::EdaxStyle {
            a: i32::MAX,
            b: i32::MAX,
            c: i32::MAX,
            q2: i32::MAX,
            q1: i32::MAX,
            q0: i32::MAX,
        },
        (3, 64),
        (0, 64),
    )
    .unwrap();
    for depth in [3, 64] {
        for empties in [0, 64] {
            let _ = pc.high(EVAL_BOUND, depth, empties);
            let _ = pc.high(INF, depth, empties);
            let _ = pc.low(-EVAL_BOUND, depth, empties);
            let _ = pc.low(-INF, depth, empties);
        }
    }
}
