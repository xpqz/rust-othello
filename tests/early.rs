use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use othello::agents::{Agent, clock_agent, search_agent};
use othello::board::{Board, empty_count, is_terminal, legal_moves, pass, play};
use othello::pattern::evaluate;
use othello::rng::Rng;
use othello::search::{
    ClockPolicy, Early, NO_DEADLINE, SearchConfig, SearchLimits, allot, allot_with, search_limited,
    stops,
};

const T: u32 = 18;

/// Lowest-square plies from the start until `empties` remain and the mover
/// has a legal move.
fn line(empties: u32) -> Board {
    let mut board = Board::START;
    while empty_count(board) > empties || legal_moves(board.me, board.opp) == 0 {
        assert!(!is_terminal(board));
        let moves = legal_moves(board.me, board.opp);
        board = if moves == 0 {
            pass(board)
        } else {
            play(board, moves.trailing_zeros() as u8)
        };
    }
    board
}

fn heuristic() -> SearchConfig {
    SearchConfig {
        endgame_threshold: 0,
        ..SearchConfig::standard()
    }
}

fn early(stable: u8, decided: u16, share: u8) -> Early {
    Early {
        stable,
        decided,
        share,
    }
}

/// No time limits at all: only `early` and `max_depth` end the search.
fn untimed(max_depth: u32, early: Early) -> SearchLimits {
    SearchLimits {
        max_depth,
        time_ms: NO_DEADLINE,
        max_nodes: 0,
        start_ms: NO_DEADLINE,
        early,
    }
}

const NONE: Duration = Duration::ZERO;

#[test]
fn early_off_default() {
    assert_eq!(SearchLimits::new(64, 3000, 0).early, Early::OFF);
    assert_eq!(SearchLimits::depth(5).early, Early::OFF);
    assert_eq!(Early::OFF, early(0, 0, 0));
}

#[test]
fn stops_stable() {
    let rule = early(3, 0, 0);
    assert!(stops(NONE, 1000, &rule, &[(5, 0), (5, 10), (5, -10)]));
    assert!(!stops(NONE, 1000, &rule, &[(5, 0), (6, 0), (5, 0)]));
    assert!(!stops(NONE, 1000, &rule, &[(5, 0), (5, 0)]), "too short");
    // A broken streak needs `stable` new entries.
    let rebuilt = [(5, 0), (5, 0), (5, 0), (6, 0), (6, 0)];
    assert!(!stops(NONE, 1000, &rule, &rebuilt));
    assert!(stops(
        NONE,
        1000,
        &rule,
        &[rebuilt.as_slice(), &[(6, 0)]].concat()
    ));
    assert!(!stops(NONE, 1000, &Early::OFF, &[(5, 0); 10]));
}

#[test]
fn stops_decided() {
    let rule = early(0, 2000, 0);
    for score in [2000, -2000, 6400, -6400] {
        assert!(stops(NONE, 1000, &rule, &[(5, 0), (6, score)]), "{score}");
    }
    for score in [1999, -1999, 0] {
        assert!(
            !stops(NONE, 1000, &rule, &[(5, 3000), (6, score)]),
            "{score}"
        );
    }
    assert!(!stops(NONE, 1000, &rule, &[]));
}

/// The share is measured on the `Duration` since entry, at nanosecond
/// precision, against `min(share, 100)`% of `start_ms`.
#[test]
fn stops_share() {
    let settled = [(5, 3000)];
    let rule = early(0, 2000, 25);
    assert!(!stops(
        Duration::from_nanos(249_999_999),
        1000,
        &rule,
        &settled
    ));
    assert!(stops(Duration::from_millis(250), 1000, &rule, &settled));
    assert!(stops(NONE, 1000, &early(0, 2000, 0), &settled));
    let over = early(0, 2000, 200);
    assert!(!stops(
        Duration::from_nanos(999_999_999),
        1000,
        &over,
        &settled
    ));
    assert!(stops(Duration::from_secs(1), 1000, &over, &settled));
    // The whole public `start_ms` range, `NO_DEADLINE` included.
    let long = Duration::from_secs(1 << 40);
    assert!(!stops(long, NO_DEADLINE, &rule, &settled));
    assert!(stops(NONE, NO_DEADLINE, &early(0, 2000, 0), &settled));
}

/// Best move and score after each depth, as iterative deepening sees them.
fn per_depth(board: Board, depths: u32) -> Vec<(u8, i32)> {
    (1..=depths)
        .map(|d| {
            let r = search_limited(board, SearchLimits::depth(d), &heuristic());
            (r.best, r.score)
        })
        .collect()
}

/// Stable 2, no time limits: the search stops after the first iteration
/// whose best move repeats the previous one, and not before.
#[test]
fn search_stable() {
    let board = line(40);
    let history = per_depth(board, 12);
    let depth = (2..=12)
        .find(|&d| history[d as usize - 1].0 == history[d as usize - 2].0)
        .expect("a repeated best move by depth 12") as u32;
    let result = search_limited(board, untimed(depth + 2, early(2, 0, 0)), &heuristic());
    let plain = search_limited(board, SearchLimits::depth(depth), &heuristic());
    assert_eq!(result.depth, depth);
    assert_eq!((result.best, result.nodes), (plain.best, plain.nodes));
}

/// A decided threshold stops after the first iteration reaching it.
#[test]
fn search_decided() {
    let board = line(40);
    let history = per_depth(board, 4);
    let first = history[0].1.unsigned_abs() as u16;
    let stopped = search_limited(board, untimed(4, early(0, first.max(1), 0)), &heuristic());
    let reached = history
        .iter()
        .position(|h| h.1.unsigned_abs() >= u32::from(first.max(1)));
    assert_eq!(stopped.depth, reached.unwrap() as u32 + 1);
    let above = history.iter().map(|h| h.1.unsigned_abs()).max().unwrap() as u16 + 1;
    let full = search_limited(board, untimed(4, early(0, above, 0)), &heuristic());
    assert_eq!(full.depth, 4, "never decided, so never stopped early");
}

#[test]
fn exact_ignores_early() {
    let limits = SearchLimits {
        early: early(1, 1, 0),
        ..SearchLimits::depth(64)
    };
    let result = search_limited(line(10), limits, &SearchConfig::standard());
    assert!(result.solved);
}

/// The first seeded random position with exactly one legal move and
/// `empties` in `range`.
fn lone(range: std::ops::RangeInclusive<u32>) -> (Board, u8) {
    for seed in 0..10_000 {
        let mut rng = Rng::new(seed);
        let mut board = Board::START;
        while !is_terminal(board) {
            let moves = legal_moves(board.me, board.opp);
            if moves.count_ones() == 1 && range.contains(&empty_count(board)) {
                return (board, moves.trailing_zeros() as u8);
            }
            board = if moves == 0 {
                pass(board)
            } else {
                let squares: Vec<u8> = (0..64).filter(|s| moves & (1 << s) != 0).collect();
                play(
                    board,
                    squares[(rng.next_u64() % squares.len() as u64) as usize],
                )
            };
        }
    }
    panic!("no lone move in 10,000 games");
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn counting(board: Board) -> i32 {
    CALLS.fetch_add(1, Ordering::Relaxed);
    evaluate(board)
}

/// Evaluator calls made by one `choose`.
fn calls(agent: &mut dyn Agent, board: Board, want: u8) -> usize {
    let before = CALLS.load(Ordering::Relaxed);
    assert_eq!(agent.choose(board), Some(want));
    CALLS.load(Ordering::Relaxed) - before
}

/// A clocked agent with `forced` plays a lone move without evaluating,
/// once it has a clock; before that, and without `forced`, it searches.
#[test]
fn forced_move() {
    let (board, only) = lone(30..=50);
    let config = SearchConfig {
        eval: counting,
        ..SearchConfig::standard()
    };
    let fixed = SearchLimits::depth(3);
    let forced = ClockPolicy::variant(1).expect("variant 1");
    assert!(forced.forced());
    let mut agent = clock_agent(config, fixed, forced);
    assert!(
        calls(&mut agent, board, only) > 0,
        "before the first clock it searches"
    );
    agent.clock(10_000);
    assert_eq!(calls(&mut agent, board, only), 0, "forced: no search");
    let plain = ClockPolicy::variant(0).unwrap();
    assert!(!plain.forced());
    let mut unforced = clock_agent(config, fixed, plain);
    unforced.clock(10_000);
    assert!(calls(&mut unforced, board, only) > 0);
    let mut flat = search_agent(config, fixed);
    flat.clock(10_000);
    assert!(calls(&mut flat, board, only) > 0);
}

fn forbidden(_: Board) -> i32 {
    panic!("a forced move must not evaluate")
}

/// The shortcut also applies at exact empties.
#[test]
fn forced_exact() {
    let (board, only) = lone(4..=T);
    let config = SearchConfig {
        eval: forbidden,
        ..SearchConfig::standard()
    };
    let mut agent = clock_agent(
        config,
        SearchLimits::depth(3),
        ClockPolicy::variant(2).unwrap(),
    );
    agent.clock(10_000);
    assert_eq!(agent.choose(board), Some(only));
}

#[test]
fn policy_variants() {
    for k in 0..4 {
        assert!(ClockPolicy::variant(k).is_some(), "{k}");
    }
    assert!(ClockPolicy::variant(4).is_none());
    let forced: Vec<bool> = (0..4)
        .map(|k| ClockPolicy::variant(k).unwrap().forced())
        .collect();
    assert_eq!(forced, [false, true, true, true]);
}

/// Variant 0 is `allot`, on every approved example and range.
#[test]
fn variant_zero_allot() {
    let zero = ClockPolicy::variant(0).unwrap();
    for empties in [10, 18, 19, 30, 40, 44, 45, 60] {
        let board = line(empties);
        for r in [0, 1, 3, 15, 1000, 16_000, 35_000, 52_000, u64::MAX] {
            let (want, got) = (allot(board, r, T), allot_with(board, r, T, zero));
            assert_eq!(
                (got.start_ms, got.time_ms, got.early),
                (want.start_ms, want.time_ms, Early::OFF),
                "e {empties} R {r}"
            );
        }
    }
}

/// Each variant's allocation and exits on one example: e 40, R 35 s.
#[test]
fn variant_limits() {
    let board = line(40);
    let want = [
        (3500, 7000, Early::OFF),
        (3500, 7000, early(3, 0, 25)),
        (3500, 7000, early(3, 2000, 25)),
        // Weight 2, reserve 4, cap 3: t = 2 × 35,000 / 15 = 4,666;
        // time = min(3t, R/4) = 8,750.
        (4666, 8750, early(3, 2000, 25)),
    ];
    for (k, want) in want.into_iter().enumerate() {
        let got = allot_with(board, 35_000, T, ClockPolicy::variant(k).unwrap());
        assert_eq!((got.start_ms, got.time_ms, got.early), want, "variant {k}");
    }
    let exact = allot_with(line(T), 16_000, T, ClockPolicy::variant(3).unwrap());
    assert_eq!(
        (exact.start_ms, exact.time_ms),
        (2000, 2000),
        "exact branch unchanged"
    );
}
