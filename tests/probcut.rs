//! ProbCut v2 (docs/design/probcut2.md): the check, its arithmetic, its
//! domain, the off switch and table policy.

use othello::board::{Board, empty_count, legal_moves, pass, play};
use othello::score::{EVAL_BOUND, INF};
use othello::search::{
    Ctx, NO_DEADLINE, ProbCut, SearchConfig, SearchLimits, SearchStatus, negamax, probcut,
    probcut_bounds, search_tt, shallow_depth, solve,
};
use othello::tt::{MAX_ENTRIES, Table, zobrist};

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

/// Lowest-square moves (passing when needed) down to `empties`.
fn played(mut board: Board, empties: u32) -> Board {
    while empty_count(board) > empties {
        let moves = legal_moves(board.me, board.opp);
        board = if moves == 0 {
            pass(board)
        } else {
            play(board, moves.trailing_zeros() as u8)
        };
    }
    board
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

/// The full-width oracle configuration, explicitly off.
fn off() -> SearchConfig {
    SearchConfig {
        probcut: None,
        ..SearchConfig::standard()
    }
}

/// Full-width value of `board` at depth `depth`, no table.
fn value(board: Board, depth: u32) -> i32 {
    let config = off();
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
        assert!(ctx.nodes > 0, "probe nodes count in the context");
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
    let v = value(board, S);
    for model in [
        tight(D + 1, D + 4, 0, 64),
        tight(1, D - 1, 0, 64),
        tight(D, D, e + 1, 64),
        tight(D, D, 0, e - 1),
        tight(D + 1, D, 0, 64),
    ] {
        let config = with(model);
        let mut ctx = Ctx::new(&config);
        assert_eq!(
            probcut(board, D, v - 1, v, &mut ctx),
            Some(None),
            "{model:?}"
        );
        assert_eq!((ctx.nodes, ctx.probcut_tries), (0, 0), "{model:?}");
    }
    // The rectangle is inclusive: e_min = e_max = the node's empties checks.
    let config = with(tight(D, D, e, e));
    let mut ctx = Ctx::new(&config);
    assert_eq!(probcut(board, D, v - 1, v, &mut ctx), Some(Some(v)));
    // Outside the rectangle at huge depths, with extreme coefficients: no
    // arithmetic and no node.
    let config = with(ProbCut {
        t: u32::MAX,
        s0: i32::MAX,
        s1: i32::MIN,
        s2: i32::MAX,
        a_milli: i32::MIN,
        b: i32::MAX,
        ..tight(3, 12, 0, 64)
    });
    for d in [13, 1000, u32::MAX] {
        let mut ctx = Ctx::new(&config);
        assert_eq!(probcut(board, d, -INF, INF, &mut ctx), Some(None));
        assert_eq!((ctx.nodes, ctx.probcut_tries), (0, 0));
    }
}

#[test]
fn unreachable_thresholds() {
    let board = midgame()[0];
    let config = with(tight(D, D, 0, 64));
    // Infinite bounds put H above and L below ±EVAL_BOUND: no probe runs.
    let mut ctx = Ctx::new(&config);
    assert_eq!(probcut(board, D, -INF, INF, &mut ctx), Some(None));
    assert_eq!(ctx.nodes, 0);
    // Each side skips on its own: with zero margin, β just past EVAL_BOUND
    // with α at -INF, and α just past -EVAL_BOUND with β at INF.
    for (alpha, beta) in [(-INF, EVAL_BOUND + 1), (-EVAL_BOUND - 1, INF)] {
        let mut ctx = Ctx::new(&config);
        assert_eq!(probcut(board, D, alpha, beta, &mut ctx), Some(None));
        assert_eq!(ctx.nodes, 0, "window {alpha} {beta}");
    }
    // At the bounds only one side is out of reach; the other cuts validly.
    let mut ctx = Ctx::new(&config);
    let top = probcut(board, D, EVAL_BOUND, EVAL_BOUND + 1, &mut ctx);
    assert_eq!((top, ctx.probcut_cuts), (Some(Some(EVAL_BOUND)), [0, 1]));
    let mut ctx = Ctx::new(&config);
    let bottom = probcut(board, D, -EVAL_BOUND - 1, -EVAL_BOUND, &mut ctx);
    assert_eq!(
        (bottom, ctx.probcut_cuts),
        (Some(Some(-EVAL_BOUND)), [1, 0])
    );
}

#[test]
fn far_thresholds() {
    // An intercept at ±EVAL_BOUND puts H below -EVAL_BOUND (easy to reach)
    // or L above EVAL_BOUND (easy to fail low): both still probe and cut.
    let model = |b| ProbCut {
        b,
        ..tight(D, D, 0, 64)
    };
    let config = with(model(EVAL_BOUND));
    assert_eq!(probcut_bounds(&model(EVAL_BOUND), D, 60, -2, -1).0, -6401);
    let mut ctx = Ctx::new(&config);
    let high = probcut(Board::START, D, -2, -1, &mut ctx);
    assert_eq!((high, ctx.probcut_cuts), (Some(Some(-1)), [1, 0]));
    let config = with(model(-EVAL_BOUND));
    assert_eq!(probcut_bounds(&model(-EVAL_BOUND), D, 60, 1, 2).1, 6401);
    let mut ctx = Ctx::new(&config);
    let low = probcut(Board::START, D, 1, 2, &mut ctx);
    assert_eq!((low, ctx.probcut_cuts), (Some(Some(1)), [0, 1]));
}

#[test]
fn nested_probes() {
    // Constant evaluation, an opening position (no game end within 12
    // plies), depths 3–12 supported, zero margin: the depth-12 check probes
    // depth 6, whose node checks again and probes depth 2. Both cut high at
    // 0. With ProbCut suppressed inside probes there would be one try.
    fn zero(_: Board) -> i32 {
        0
    }
    let config = SearchConfig {
        eval: zero,
        ..with(tight(3, 12, 0, 64))
    };
    let mut ctx = Ctx::new(&config);
    assert_eq!(probcut(Board::START, 12, -1, 0, &mut ctx), Some(Some(0)));
    assert_eq!((ctx.probcut_tries, ctx.probcut_cuts), (2, [2, 0]));
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
        for gap in [1, 4, 6] {
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

#[test]
fn cancelled_parent() {
    // Stopped inside the probe of a node `negamax` searches: the stop
    // propagates, the probe's nodes count against the same budget, and the
    // node stores nothing.
    let board = midgame()[0];
    let config = with(tight(D, D, 0, 64));
    let v = value(board, S);
    let mut table = Table::new(1 << 16);
    {
        let mut ctx = Ctx::new(&config);
        ctx.set_table(&mut table);
        ctx.set_node_limit(3);
        assert_eq!(negamax(board, D, v - 1, v, &mut ctx), None);
        assert_eq!(ctx.probcut_tries, 1, "the stop came inside the probe");
    }
    assert!(table.probe(zobrist(board)).is_none());
}

#[test]
fn root_contract() {
    // With ProbCut on, a node budget that stops iteration 6 returns
    // iteration 5's move and score as Partial; one node gives the fallback.
    let on = with(tight(3, 12, 0, 64));
    for board in midgame() {
        let fresh = || Table::new(1 << 18);
        let five = search_tt(board, SearchLimits::depth(5), &on, &mut fresh());
        let six = search_tt(board, SearchLimits::depth(6), &on, &mut fresh());
        assert!(six.nodes > five.nodes + 1);
        let limits = SearchLimits::new(64, NO_DEADLINE, five.nodes + 1);
        let cut = search_tt(board, limits, &on, &mut fresh());
        assert_eq!((cut.best, cut.score, cut.depth), (five.best, five.score, 5));
        assert_eq!(cut.status, SearchStatus::Partial);
        let limits = SearchLimits::new(64, NO_DEADLINE, 1);
        let none = search_tt(board, limits, &on, &mut fresh());
        assert_eq!(none.status, SearchStatus::Fallback);
    }
}

fn ceil128(n: i128, k: i128) -> i128 {
    n.div_euclid(k) + i128::from(n.rem_euclid(k) != 0)
}

fn reference_sigma(p: &ProbCut, d: u32, e: u32) -> i128 {
    let (d, e) = (i128::from(d.min(64)), i128::from(e.min(64)));
    let raw = i128::from(p.s0) + i128::from(p.s1) * e + i128::from(p.s2) * d;
    ceil128(raw, 100).clamp(1, 2 * i128::from(EVAL_BOUND))
}

/// Exact reference for the thresholds, in i128.
fn reference(p: &ProbCut, d: u32, e: u32, alpha: i32, beta: i32) -> (i128, i128) {
    let sigma = reference_sigma(p, d, e);
    let m = ceil128(i128::from(p.t.min(400)) * sigma, 100);
    let a = i128::from(p.a_milli.clamp(500, 1500));
    let b = i128::from(p.b.clamp(-EVAL_BOUND, EVAL_BOUND));
    let high = ceil128((i128::from(beta) + m - b) * 1000, a);
    let low = ((i128::from(alpha) - m - b) * 1000).div_euclid(a);
    (high, low)
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
                        assert_eq!(i128::from(p.sigma(d, e)), reference_sigma(&p, d, e));
                    }
                }
            }
        }
    }
}

#[test]
fn independent_fields() {
    // Each coefficient alone, mixed signs, fractional σ, and slope and
    // intercept varied apart, so that a swapped or misread field changes
    // the result.
    let models = [
        (150, 30_000, 0, 0, 1000, 0),
        (150, 0, 777, 0, 1000, 0),
        (150, 0, 0, 1_234, 1000, 0),
        (150, 20_050, -333, 1_001, 1000, 0),
        (150, -500, 1_000, -20, 1000, 0),
        (90, 25_001, 101, 99, 800, 0),
        (90, 25_001, 101, 99, 1000, 250),
        (90, 25_001, 101, 99, 1250, -250),
        (230, 41, 3, 7, 600, 37),
        (230, 41, 3, 7, 1400, -37),
    ];
    for (t, s0, s1, s2, a_milli, b) in models {
        let p = ProbCut {
            t,
            s0,
            s1,
            s2,
            a_milli,
            b,
            ..tight(1, 64, 0, 64)
        };
        for (d, e) in [(3, 21), (7, 30), (12, 47), (14, 56)] {
            assert_eq!(i128::from(p.sigma(d, e)), reference_sigma(&p, d, e));
            for (alpha, beta) in [(-300, -299), (17, 18), (-2000, 2000)] {
                let (high, low) = probcut_bounds(&p, d, e, alpha, beta);
                let want = reference(&p, d, e, alpha, beta);
                assert_eq!(
                    (i128::from(high), i128::from(low)),
                    want,
                    "{p:?} {d} {e} {alpha} {beta}"
                );
            }
        }
    }
    // Depth and empties coefficients are not interchangeable.
    let p = ProbCut {
        s1: 700,
        ..tight(1, 64, 0, 64)
    };
    let swapped = ProbCut {
        s1: 0,
        s2: 700,
        ..p
    };
    assert_eq!((p.sigma(3, 40), swapped.sigma(3, 40)), (281, 22));
    // Slope and intercept are not interchangeable.
    let q = ProbCut {
        a_milli: 800,
        b: 1000,
        ..tight(1, 64, 0, 64)
    };
    let r = ProbCut {
        a_milli: 1000,
        b: 800,
        ..tight(1, 64, 0, 64)
    };
    assert_eq!(probcut_bounds(&q, 6, 30, 0, 100).0, -1125);
    assert_eq!(probcut_bounds(&r, 6, 30, 0, 100).0, -700);
}

#[test]
fn empty_rectangle_off() {
    // A model whose rectangle is empty changes nothing: same values, nodes, moves.
    let empty = with(tight(9, 8, 0, 64));
    for board in midgame() {
        let limits = SearchLimits::depth(7);
        let a = search_tt(board, limits, &off(), &mut Table::new(1 << 16));
        let b = search_tt(board, limits, &empty, &mut Table::new(1 << 16));
        assert_eq!(a, b);
    }
}

#[test]
fn standard_is_off() {
    assert!(SearchConfig::standard().probcut.is_none());
}

#[test]
fn cleared_table_full() {
    let on = with(tight(3, 12, 0, 64));
    let mut table = Table::new(1 << 18);
    for board in midgame() {
        let limits = SearchLimits::depth(8);
        let fresh = search_tt(board, limits, &off(), &mut Table::new(1 << 18));
        search_tt(board, limits, &on, &mut table);
        table.clear();
        assert_eq!(search_tt(board, limits, &off(), &mut table), fresh);
        table.clear();
    }
}

#[test]
fn selective_search_cuts() {
    // With a permissive model the search does cut, and still returns a legal move.
    let on = with(tight(3, 12, 0, 64));
    for board in midgame() {
        let limits = SearchLimits::depth(8);
        let full = search_tt(board, limits, &off(), &mut Table::new(1 << 18));
        let result = search_tt(board, limits, &on, &mut Table::new(1 << 18));
        assert!(legal_moves(board.me, board.opp) & (1u64 << result.best) != 0);
        assert!(
            result.nodes < full.nodes,
            "{} >= {}",
            result.nodes,
            full.nodes
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
    // A model eligible at the solver's nominal depth 2 in heuristic mode
    // (gap 1: shallow depth 1) never tries in exact mode: identical results
    // and nodes at 5–12 empties (corpus), 14 and 18 (played from the corpus
    // position with 20).
    let on = with(ProbCut {
        shallow: 1,
        ..tight(1, 64, 0, 64)
    });
    let board20 = corpus()
        .into_iter()
        .find(|b| empty_count(*b) == 20)
        .unwrap();
    let mut boards: Vec<Board> = corpus()
        .into_iter()
        .filter(|board| empty_count(*board) <= 12)
        .take(8)
        .collect();
    boards.push(played(board20, 18));
    boards.push(played(board20, 14));
    assert_eq!(empty_count(boards[boards.len() - 1]), 14);
    for board in boards {
        // Eligible in heuristic mode at depth 2.
        let mut ctx = Ctx::new(&on);
        negamax(board, 2, -INF, INF, &mut ctx);
        assert!(ctx.probcut_tries > 0);
        // Never tried by the solver.
        let mut ctx = Ctx::new(&on);
        let solved = solve(board, -INF, INF, &mut ctx);
        assert_eq!(ctx.probcut_tries, 0);
        let config = off();
        assert_eq!(solved, solve(board, -INF, INF, &mut Ctx::new(&config)));
        let limits = SearchLimits::depth(64);
        let a = search_tt(board, limits, &off(), &mut Table::new(1 << 18));
        let b = search_tt(board, limits, &on, &mut Table::new(1 << 18));
        assert_eq!(a, b);
        assert!(a.solved);
    }
}
