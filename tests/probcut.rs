//! ProbCut v2 (docs/design/probcut2.md): the check, its arithmetic, its
//! domain, the off switch and table policy.

use othello::board::{Board, empty_count, legal_moves, pass, play};
use othello::score::{EVAL_BOUND, INF};
use othello::search::{
    Ctx, ProbCut, SearchConfig, SearchLimits, negamax, probcut, probcut_bounds, search_tt,
    shallow_depth,
};
use othello::tt::{MAX_ENTRIES, Table};

fn corpus() -> Vec<Board> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/bench/corpus.txt");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let me = u64::from_str_radix(parts.next()?, 16).ok()?;
            let opp = u64::from_str_radix(parts.next()?, 16).ok()?;
            Some(Board { me, opp })
        })
        .filter(|board| legal_moves(board.me, board.opp) != 0)
        .collect()
}

fn midgame() -> Vec<Board> {
    corpus()
        .into_iter()
        .filter(|board| empty_count(*board) >= 24)
        .collect()
}

/// σ = 1 centidisc, t = 0, slope 1, intercept 0: the thresholds are the window.
fn tight(d_min: u32, d_max: u32, e_min: u32, e_max: u32) -> ProbCut {
    ProbCut {
        t: 0,
        s0: 100,
        s1: 0,
        s2: 0,
        a_milli: 1000,
        b: 0,
        shallow: 0,
        d_min,
        d_max,
        e_min,
        e_max,
    }
}

fn with(probcut: ProbCut) -> SearchConfig {
    SearchConfig {
        probcut: Some(probcut),
        ..SearchConfig::standard()
    }
}

/// Full-width value of `board` at depth `depth`, no table.
fn value(board: Board, depth: u32) -> i32 {
    let config = SearchConfig::standard();
    negamax(board, depth, -INF, INF, &mut Ctx::new(&config)).unwrap()
}

// Depth 6 uses shallow depth 2 under rule 0; with the rectangle at depth 6
// only, nodes inside the probe (depth <= 2) and children (depth 5) never
// check, so the probe's verdict is the full-width depth-2 value's.
const D: u32 = 6;
const S: u32 = 2;

#[test]
fn cut_high() {
    for board in midgame() {
        let config = with(tight(D, D, 0, 64));
        let v = value(board, S);
        let mut ctx = Ctx::new(&config);
        assert_eq!(probcut(board, D, v - 1, v, &mut ctx), Some(Some(v)));
        assert_eq!((ctx.probcut_tries, ctx.probcut_cuts), (1, [1, 0]));
    }
}

#[test]
fn cut_low() {
    // The high query's valid opposite return: window (v, v + 1) cuts low at v.
    for board in midgame() {
        let config = with(tight(D, D, 0, 64));
        let v = value(board, S);
        let mut ctx = Ctx::new(&config);
        assert_eq!(probcut(board, D, v, v + 1, &mut ctx), Some(Some(v)));
        assert_eq!((ctx.probcut_tries, ctx.probcut_cuts), (1, [0, 1]));
        // The low query's valid opposite return: window (v - 1, v) cuts high at v.
        let mut ctx = Ctx::new(&config);
        assert_eq!(probcut(board, D, v - 1, v, &mut ctx), Some(Some(v)));
        assert_eq!(ctx.probcut_cuts, [1, 0]);
    }
}

#[test]
fn no_cut() {
    for board in midgame() {
        let config = with(tight(D, D, 0, 64));
        let v = value(board, S);
        let mut ctx = Ctx::new(&config);
        assert_eq!(probcut(board, D, v - 10, v + 10, &mut ctx), Some(None));
        assert_eq!((ctx.probcut_tries, ctx.probcut_cuts), (1, [0, 0]));
    }
}

#[test]
fn margin_rounds_up() {
    // σ = 10, t = 1.00: m = 10. H = β + 10 reaches v only from β = v - 10.
    for board in midgame() {
        let model = ProbCut {
            t: 100,
            s0: 1000,
            ..tight(D, D, 0, 64)
        };
        let config = with(model);
        let v = value(board, S);
        let mut ctx = Ctx::new(&config);
        assert_eq!(
            probcut(board, D, v - 11, v - 10, &mut ctx),
            Some(Some(v - 10))
        );
        let mut ctx = Ctx::new(&config);
        assert_eq!(probcut(board, D, v - 10, v - 9, &mut ctx), Some(None));
    }
}

#[test]
fn outside_rectangle() {
    let board = midgame()[0];
    let e = empty_count(board);
    for model in [
        tight(D + 1, D + 4, 0, 64),
        tight(1, D - 1, 0, 64),
        tight(D, D, e + 1, 64),
        tight(D, D, 0, e - 1),
        tight(D + 1, D, 0, 64),
    ] {
        let config = with(model);
        let v = value(board, S);
        let mut ctx = Ctx::new(&config);
        assert_eq!(
            probcut(board, D, v - 1, v, &mut ctx),
            Some(None),
            "{model:?}"
        );
        assert_eq!((ctx.nodes, ctx.probcut_tries), (0, 0), "{model:?}");
    }
}

#[test]
fn unreachable_thresholds() {
    // Infinite bounds put H above and L below ±EVAL_BOUND: no probe runs.
    let board = midgame()[0];
    let config = with(tight(D, D, 0, 64));
    let mut ctx = Ctx::new(&config);
    assert_eq!(probcut(board, D, -INF, INF, &mut ctx), Some(None));
    assert_eq!(ctx.nodes, 0);
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        probcut(board, D, EVAL_BOUND, EVAL_BOUND + 1, &mut ctx),
        Some(None)
    );
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        probcut(board, D, -EVAL_BOUND - 1, -EVAL_BOUND, &mut ctx),
        Some(None)
    );
}

#[test]
fn shallow_rules() {
    let rule = |shallow| ProbCut {
        shallow,
        ..tight(1, 64, 0, 64)
    };
    for d in 0..=40 {
        let v1 = (d >= 3).then(|| 2 * (d / 4) + (d & 1));
        assert_eq!(shallow_depth(&rule(0), d), v1, "rule 0 d {d}");
        for gap in [4, 6] {
            let want = (d > gap).then(|| d - gap);
            assert_eq!(shallow_depth(&rule(gap), d), want, "gap {gap} d {d}");
        }
    }
    // A depth without a valid shallow depth runs no check.
    let board = midgame()[0];
    let config = with(ProbCut {
        shallow: 6,
        ..tight(1, 64, 0, 64)
    });
    let mut ctx = Ctx::new(&config);
    assert_eq!(probcut(board, 5, 0, 1, &mut ctx), Some(None));
    assert_eq!((ctx.nodes, ctx.probcut_tries), (0, 0));
}

#[test]
fn cancelled_probe() {
    let board = midgame()[0];
    let config = with(tight(D, D, 0, 64));
    let v = value(board, S);
    let mut ctx = Ctx::new(&config);
    ctx.set_node_limit(1);
    assert_eq!(probcut(board, D, v - 1, v, &mut ctx), None);
}

/// Exact reference for the thresholds, in i128.
fn reference(p: &ProbCut, d: u32, e: u32, alpha: i32, beta: i32) -> (i128, i128) {
    let ceil = |n: i128, k: i128| n.div_euclid(k) + i128::from(n.rem_euclid(k) != 0);
    let raw =
        i128::from(p.s0) + i128::from(p.s1) * i128::from(e) + i128::from(p.s2) * i128::from(d);
    let sigma = ceil(raw, 100).clamp(1, 2 * i128::from(EVAL_BOUND));
    let t = i128::from(p.t.min(400));
    let m = ceil(t * sigma, 100);
    let a = i128::from(p.a_milli.clamp(500, 1500));
    let b = i128::from(p.b.clamp(-EVAL_BOUND, EVAL_BOUND));
    let high = ceil((i128::from(beta) + m - b) * 1000, a);
    let low = (i128::from(alpha) - m - b) * 1000;
    (high, low.div_euclid(a))
}

#[test]
fn threshold_limits() {
    let extremes = [
        i32::MIN,
        -1_000_000,
        -6401,
        -150,
        -1,
        0,
        1,
        99,
        101,
        6401,
        i32::MAX,
    ];
    let windows = [
        (-INF, INF),
        (-1, 0),
        (0, 1),
        (-6400, 6400),
        (37, 38),
        (-38, -37),
    ];
    for &s in &extremes {
        for &ab in &extremes {
            for t in [0, 1, 150, 400, 401, u32::MAX] {
                for (d, e) in [(0, 0), (6, 30), (64, 64)] {
                    for &(alpha, beta) in &windows {
                        let p = ProbCut {
                            t,
                            s0: s,
                            s1: s,
                            s2: s,
                            a_milli: ab,
                            b: ab,
                            ..tight(1, 64, 0, 64)
                        };
                        let (high, low) = probcut_bounds(&p, d, e, alpha, beta);
                        let want = reference(&p, d, e, alpha, beta);
                        assert_eq!(
                            (i128::from(high), i128::from(low)),
                            want,
                            "{p:?} d {d} e {e} window {alpha} {beta}"
                        );
                        let sigma = p.sigma(d, e);
                        assert!((1..=2 * i64::from(EVAL_BOUND)).contains(&sigma));
                    }
                }
            }
        }
    }
}

#[test]
fn empty_rectangle_is_off() {
    // A model whose rectangle is empty changes nothing: same values, nodes, moves.
    let off = SearchConfig::standard();
    let empty = with(tight(9, 8, 0, 64));
    for board in midgame() {
        let a = search_tt(
            board,
            SearchLimits::depth(7),
            &off,
            &mut Table::new(1 << 16),
        );
        let b = search_tt(
            board,
            SearchLimits::depth(7),
            &empty,
            &mut Table::new(1 << 16),
        );
        assert_eq!(a, b);
    }
}

#[test]
fn standard_is_off() {
    assert!(SearchConfig::standard().probcut.is_none());
}

#[test]
fn cleared_table_is_full_width() {
    let off = SearchConfig::standard();
    let on = with(tight(3, 12, 0, 64));
    let mut table = Table::new(1 << 18);
    for board in midgame() {
        let fresh = search_tt(
            board,
            SearchLimits::depth(8),
            &off,
            &mut Table::new(1 << 18),
        );
        search_tt(board, SearchLimits::depth(8), &on, &mut table);
        table.clear();
        assert_eq!(
            search_tt(board, SearchLimits::depth(8), &off, &mut table),
            fresh
        );
        table.clear();
    }
}

#[test]
fn selective_search_cuts() {
    // With a permissive model the search does cut, and still returns a legal move.
    let on = with(tight(3, 12, 0, 64));
    for board in midgame() {
        let off = search_tt(
            board,
            SearchLimits::depth(8),
            &SearchConfig::standard(),
            &mut Table::new(1 << 18),
        );
        let result = search_tt(board, SearchLimits::depth(8), &on, &mut Table::new(1 << 18));
        assert!(legal_moves(board.me, board.opp) & (1u64 << result.best) != 0);
        assert!(
            result.nodes < off.nodes,
            "{} >= {}",
            result.nodes,
            off.nodes
        );
    }
}

#[test]
fn legal_under_limits() {
    let on = with(ProbCut {
        t: 150,
        s0: 40_000,
        ..tight(3, 14, 20, 60)
    });
    for board in midgame() {
        for limits in [
            SearchLimits::new(64, 1000, 1),
            SearchLimits::new(64, 1000, 500),
            SearchLimits::new(64, 0, 0),
            SearchLimits::new(64, 20, 0),
        ] {
            let result = search_tt(board, limits, &on, &mut Table::new(MAX_ENTRIES >> 6));
            assert!(legal_moves(board.me, board.opp) & (1u64 << result.best) != 0);
        }
    }
}

#[test]
fn exact_unaffected() {
    // The solver never checks: identical results and nodes at 14 and 18 empties.
    let on = with(tight(1, 64, 0, 64));
    let off = SearchConfig::standard();
    let mut boards: Vec<Board> = corpus()
        .into_iter()
        .filter(|board| empty_count(*board) <= 14)
        .take(8)
        .collect();
    // 18 empties: two lowest-square moves from the corpus position with 20.
    let mut board = corpus()
        .into_iter()
        .find(|b| empty_count(*b) == 20)
        .unwrap();
    while empty_count(board) > 18 {
        let moves = legal_moves(board.me, board.opp);
        board = if moves == 0 {
            pass(board)
        } else {
            play(board, moves.trailing_zeros() as u8)
        };
    }
    boards.push(board);
    assert!(boards.len() >= 5);
    for board in boards {
        let a = search_tt(
            board,
            SearchLimits::depth(64),
            &off,
            &mut Table::new(1 << 18),
        );
        let b = search_tt(
            board,
            SearchLimits::depth(64),
            &on,
            &mut Table::new(1 << 18),
        );
        assert_eq!(a, b);
        assert!(a.solved);
    }
}
