use othello::board::{Board, empty_count, legal_moves};
use othello::eval::evaluate;
use othello::probcut::{ProbCut, SigmaModel, shallow_depth};
use othello::score::{EVAL_BOUND, INF};
use othello::search::{Ctx, SearchConfig, SearchLimits, negamax, probcut_fit, search_limited};
use othello::tt::Table;

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
    let table = SigmaModel::StageTable {
        buckets: [(0, 0); 6],
    };
    for sigma in [linear, table] {
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
    assert_eq!(pc.high(-500, 8, 30), Some(-422));
    assert_eq!(pc.high(780, 8, 30), Some(1000));
    assert_eq!(pc.low(-400, 8, 30), Some(-645));
    assert_eq!(pc.low(100, 8, 30), Some(-89));
    assert_eq!(pc.low(400, 8, 30), Some(244));
    assert_eq!(pc.low(-720, 8, 30), Some(-1000));
}

#[test]
fn fractional_margin_rounds_up() {
    let pc = ProbCut::new(
        150,
        900,
        30,
        SigmaModel::Linear {
            s0: 33,
            gap: 0,
            empty: 0,
        },
        (3, 12),
        (18, 52),
    )
    .unwrap();
    assert_eq!(pc.high(500, 8, 30), Some(578));
    assert_eq!(pc.low(400, 8, 30), Some(355));
}

#[test]
fn guards_skip_beyond_the_eval_bound() {
    let pc = params();
    assert_eq!(pc.high(5700, 8, 30), None);
    assert_eq!(pc.high(5640, 8, 30), Some(6400));
    assert_eq!(pc.high(5641, 8, 30), None);
    assert_eq!(pc.high(EVAL_BOUND, 8, 30), None);
    assert_eq!(pc.high(INF, 8, 30), None);
    assert_eq!(pc.low(-5580, 8, 30), Some(-6400));
    assert_eq!(pc.low(-5579, 8, 30), Some(-6399));
    assert_eq!(pc.low(-5581, 8, 30), None);
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
    assert_eq!(pc.sigma(8, 0), 140);
    assert_eq!(pc.sigma(8, 48), 280);
    assert!(pc.sigma(3, 52) >= 1);
}

#[test]
fn extreme_values_do_not_panic() {
    let pc = ProbCut::new(
        400,
        1500,
        i32::MAX,
        SigmaModel::StageTable {
            buckets: [(i32::MAX, i32::MAX); 6],
        },
        (3, 64),
        (0, 64),
    )
    .unwrap();
    for depth in [3, 64] {
        for empties in [0, 64] {
            assert_eq!(pc.high(EVAL_BOUND, depth, empties), None);
            assert_eq!(pc.high(INF, depth, empties), None);
            assert_eq!(pc.low(-EVAL_BOUND, depth, empties), None);
            assert_eq!(pc.low(-INF, depth, empties), None);
        }
    }
}

#[test]
fn accepted_bounds_and_domain_caps() {
    for (t, a) in [(50, 500), (400, 1500)] {
        assert!(ProbCut::new(t, a, 0, linear(), (3, 12), (18, 52)).is_ok());
    }
    assert!(ProbCut::new(150, 900, 0, linear(), (3, 65), (18, 52)).is_err());
    assert!(ProbCut::new(150, 900, 0, linear(), (3, 12), (0, 65)).is_err());
}

// docs/tuning/probcut_fit.log from e012aa8; refit/update deliberately if evaluator or search values change.
const FINGERPRINT: u64 = 1994198706096676413;

fn corpus() -> Vec<Board> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/bench/corpus.txt");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let me = u64::from_str_radix(parts.next().unwrap(), 16).unwrap();
            let opp = u64::from_str_radix(parts.next().unwrap(), 16).unwrap();
            Board { me, opp }
        })
        .collect()
}

fn hash(mut value: u64, item: i32) -> u64 {
    value ^= item as u32 as u64;
    value.wrapping_mul(0x0000_0100_0000_01b3)
}

fn fingerprint(eval: fn(Board) -> i32) -> u64 {
    let config = SearchConfig {
        eval,
        probcut: None,
        ..SearchConfig::standard()
    };
    let mut hash_value = 0xcbf2_9ce4_8422_2325u64;
    for board in corpus() {
        hash_value = hash(hash_value, eval(board));
        if empty_count(board) <= 12 || legal_moves(board.me, board.opp) == 0 {
            continue;
        }
        let mut table = Table::new(1 << 16);
        for depth in 1..=6 {
            let mut ctx = Ctx::new(&config);
            ctx.set_table(&mut table);
            hash_value = hash(
                hash_value,
                negamax(board, depth, -INF, INF, &mut ctx).unwrap(),
            );
        }
    }
    hash_value
}

fn perturbed(board: Board) -> i32 {
    evaluate(board) + 1
}

#[test]
fn probcut_calibration_fingerprint() {
    assert_eq!(
        fingerprint(evaluate),
        FINGERPRINT,
        "evaluator or search values changed since ProbCut was calibrated: refit (#111) and update PROBCUT_FIT, or update this fingerprint deliberately with a note in the PR"
    );
    assert_ne!(fingerprint(perturbed), FINGERPRINT);
}

#[test]
fn probcut_decision_error_tripwire() {
    let config = SearchConfig {
        probcut: Some(probcut_fit()),
        ..SearchConfig::standard()
    };
    let mut wrong = 0;
    let mut total = 0;
    for board in corpus() {
        if empty_count(board) <= 12 || legal_moves(board.me, board.opp) == 0 {
            continue;
        }
        let plain = SearchConfig {
            probcut: None,
            ..SearchConfig::standard()
        };
        let mut ctx = Ctx::new(&plain);
        let exact = negamax(board, 8, -INF, INF, &mut ctx).unwrap();
        let (alpha, beta) = (exact - 100, exact + 100);
        let mut ctx = Ctx::new(&config);
        let selective = negamax(board, 8, alpha, beta, &mut ctx).unwrap();
        total += 1;
        if selective < alpha || selective > beta {
            wrong += 1;
        }
    }
    assert!(total > 0);
    assert!((wrong as f64) / (total as f64) < 0.15, "{wrong}/{total}");
}

#[test]
fn probcut_node_reduction() {
    let plain = SearchConfig {
        probcut: None,
        ..SearchConfig::standard()
    };
    let selective = SearchConfig {
        probcut: Some(probcut_fit()),
        ..SearchConfig::standard()
    };
    let (mut full, mut cut) = (0u64, 0u64);
    for board in corpus() {
        if empty_count(board) <= 12 || legal_moves(board.me, board.opp) == 0 {
            continue;
        }
        full += search_limited(board, SearchLimits::depth(8), &plain).nodes;
        cut += search_limited(board, SearchLimits::depth(8), &selective).nodes;
    }
    assert!(cut < full, "{cut} vs {full}");
    println!(
        "probcut depth-8 node reduction {:.1}%",
        100.0 * (full - cut) as f64 / full as f64
    );
}
