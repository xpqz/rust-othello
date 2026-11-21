use std::env;
use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use othello::agents::{
    Agent, GameClock, PLAY_CLOCK_MS, PLAY_POLICY, clock_agent, play_agent, search_agent,
};
use othello::board::{Board, empty_count, final_score, is_terminal, legal_moves, pass, play};
use othello::rng::Rng;
use othello::score::terminal_score;
use othello::search::{
    CLOCK_POLICIES, Early, MAX_CLOCK_MS, NO_DEADLINE, SearchConfig, SearchLimits, allot,
    search_limited, search_tt,
};
use othello::tt::{MAX_ENTRIES, Table};

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
    assert_eq!(empty_count(board), empties);
    board
}

fn heuristic() -> SearchConfig {
    SearchConfig {
        endgame_threshold: 0,
        ..SearchConfig::standard()
    }
}

fn limits(time_ms: u64, start_ms: u64, max_depth: u32) -> SearchLimits {
    SearchLimits {
        max_depth,
        time_ms,
        max_nodes: 0,
        start_ms,
        early: Early::OFF,
    }
}

/// Runs `case` in a child copy of this test binary (`child_search`), and
/// kills and reaps it after `seconds`, so a runaway search cannot outlive
/// its test. Returns the child's `depth nodes best score` fields.
fn bounded(case: &str, seconds: u64) -> Option<Vec<i64>> {
    let mut child = Command::new(env::current_exe().unwrap())
        .args(["--exact", "child_search", "--ignored", "--nocapture"])
        .env("CLOCK_CASE", case)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(seconds) {
            child.kill().unwrap();
            child.wait().unwrap();
            return None;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    let line = out.lines().find_map(|l| l.strip_prefix("result "))?;
    Some(line.split(' ').map(|f| f.parse().unwrap()).collect())
}

/// The child side of `bounded`; does nothing unless `CLOCK_CASE` is set.
#[test]
#[ignore]
fn child_search() {
    let limits = match env::var("CLOCK_CASE").as_deref() {
        Ok("deadline") => limits(50, NO_DEADLINE, 64),
        Ok("admission") => limits(NO_DEADLINE, 50, 64),
        _ => return,
    };
    let r = search_limited(line(40), limits, &heuristic());
    println!("result {} {} {} {}", r.depth, r.nodes, r.best, r.score);
}

#[test]
fn flat_start_equal() {
    let flat = SearchLimits::new(64, 3000, 0);
    assert_eq!(flat.start_ms, flat.time_ms);
    let depth = SearchLimits::depth(5);
    assert_eq!(depth.start_ms, depth.time_ms);
    assert_eq!(depth.start_ms, NO_DEADLINE);
}

/// The in-search check reads `time_ms`: with no start cutoff, a 50 ms
/// deadline still aborts a 40-empty search bound for depth 64.
#[test]
fn deadline_aborts_search() {
    let result = bounded("deadline", 30).expect("the in-search check must read time_ms");
    assert!(result[0] < 64);
}

/// Admission reads `start_ms`: with no deadline, no iteration starts after
/// 50 ms, and the last one runs to completion.
#[test]
fn start_stops_admission() {
    let result = bounded("admission", 60).expect("admission must read start_ms");
    assert!((1..64).contains(&result[0]));
    let plain = search_limited(
        line(40),
        SearchLimits::depth(result[0] as u32),
        &heuristic(),
    );
    let want = [plain.nodes as i64, plain.best.into(), plain.score.into()];
    assert_eq!(result[1..], want, "last iteration incomplete or different");
}

#[test]
fn start_zero_fallback() {
    let result = search_limited(line(40), limits(NO_DEADLINE, 0, 3), &heuristic());
    assert_eq!(result.depth, 0);
}

/// The exact branch ignores `start_ms`.
#[test]
fn exact_ignores_start() {
    let config = SearchConfig::standard();
    let result = search_limited(line(10), limits(10_000, 0, 64), &config);
    assert!(result.solved);
}

#[test]
fn allot_exact() {
    let board = line(T);
    for (r, time) in [(16_000, 2000), (15, 1), (7, 1), (0, 1)] {
        let got = allot(board, r, T);
        assert_eq!((got.start_ms, got.time_ms), (time, time), "R {r}");
        assert_eq!((got.max_depth, got.max_nodes), (64, 0));
    }
    let deadline = 4 * allot(board, 15, T).time_ms;
    assert!(deadline <= 15 / 2);
}

#[test]
fn allot_examples() {
    let cases = [
        (60, 52_000, 2080, 4160),
        (40, 35_000, 3500, 7000),
        (19, 1000, 250, 250),
    ];
    for (empties, r, start, time) in cases {
        let got = allot(line(empties), r, T);
        assert_eq!((got.start_ms, got.time_ms), (start, time), "e {empties}");
        assert_eq!((got.max_depth, got.max_nodes), (64, 0));
    }
}

#[test]
fn allot_bounds() {
    for empties in [19, 30, 44, 45, 60] {
        let board = line(empties);
        let mut last = (0, 0);
        for r in (0..200).map(|k| k * k * 37).chain([MAX_CLOCK_MS]) {
            let got = allot(board, r, T);
            assert!(
                1 <= got.start_ms && got.start_ms <= got.time_ms,
                "e {empties} R {r}"
            );
            assert!(got.time_ms <= (r / 4).max(1), "e {empties} R {r}");
            assert!(
                got.start_ms >= last.0 && got.time_ms >= last.1,
                "not monotone at R {r}"
            );
            last = (got.start_ms, got.time_ms);
        }
    }
}

/// e = 44 with T = 18 and e = 45 with T = 19 both have m = 13; only the
/// midgame weight differs.
#[test]
fn allot_weight() {
    let mid = allot(line(44), 34_000, 18);
    let early = allot(line(45), 34_000, 19);
    assert_eq!((mid.start_ms, early.start_ms), (3000, 2000));
}

#[test]
fn allot_tiny() {
    for empties in [10, 30] {
        for r in [0, 1, 3] {
            let got = allot(line(empties), r, T);
            assert_eq!((got.start_ms, got.time_ms), (1, 1), "e {empties} R {r}");
        }
    }
}

#[test]
fn allot_clamped() {
    for empties in [10, 30, 60] {
        let board = line(empties);
        let top = allot(board, MAX_CLOCK_MS, T);
        let over = allot(board, u64::MAX, T);
        assert_eq!((over.start_ms, over.time_ms), (top.start_ms, top.time_ms));
        assert!(top.time_ms > 1);
    }
}

#[test]
fn clock_ignored_flat() {
    let flat = SearchLimits::new(64, 3000, 0);
    let mut agent = search_agent(SearchConfig::standard(), flat);
    let board = line(30);
    agent.clock(5000);
    let got = agent.limits(board);
    assert_eq!((got.time_ms, got.start_ms), (3000, 3000));
}

/// Fixed limits before any clock, then each clock update's allotment.
#[test]
fn clock_limits_follow() {
    let flat = SearchLimits::new(64, 3000, 0);
    let mut agent = clock_agent(SearchConfig::standard(), flat, &CLOCK_POLICIES[0]);
    let board = line(30);
    assert_eq!(agent.limits(board).time_ms, 3000);
    let (first, second) = (allot(board, 40_000, T), allot(board, 20_000, T));
    assert_ne!(first.time_ms, second.time_ms);
    agent.clock(40_000);
    let got = agent.limits(board);
    assert_eq!((got.start_ms, got.time_ms), (first.start_ms, first.time_ms));
    agent.clock(20_000);
    let got = agent.limits(board);
    assert_eq!(
        (got.start_ms, got.time_ms),
        (second.start_ms, second.time_ms)
    );
}

/// The exact value for the mover of playing `square`.
fn exact_after(board: Board, square: u8) -> i32 {
    let child = play(board, square);
    let config = SearchConfig::standard();
    if is_terminal(child) {
        return -terminal_score(final_score(child));
    }
    if legal_moves(child.me, child.opp) == 0 {
        return search_limited(pass(child), SearchLimits::depth(64), &config).score;
    }
    -search_limited(child, SearchLimits::depth(64), &config).score
}

/// A 16-empty position where the depth-8 fallback under `frozen` limits
/// (time 0, fresh table as in a new agent) plays a move that is strictly
/// worse than the exact optimum. Returns the board, the limits, that move
/// and the optimum value.
fn split() -> (Board, SearchLimits, u8, i32) {
    let config = SearchConfig::standard();
    let frozen = limits(0, 0, 64);
    for seed in 0..1000 {
        let mut rng = Rng::new(seed);
        let mut board = Board::START;
        while empty_count(board) > 16 && !is_terminal(board) {
            let moves = legal_moves(board.me, board.opp);
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
        if legal_moves(board.me, board.opp) == 0 {
            continue;
        }
        let quick = search_tt(board, frozen, &config, &mut Table::new(MAX_ENTRIES)).best;
        let exact = search_limited(board, SearchLimits::depth(64), &config);
        assert!(exact.solved);
        if exact_after(board, quick) < exact.score {
            return (board, frozen, quick, exact.score);
        }
    }
    panic!("no distinguishing 16-empty position in 1,000 seeds")
}

/// `choose` plays from the limits `limits` reports: the frozen limits'
/// fallback move before the clock, an optimal move once the clock allows a
/// full solve.
#[test]
fn clock_choose_uses() {
    let (board, frozen, quick, optimum) = split();
    let mut agent = clock_agent(SearchConfig::standard(), frozen, &CLOCK_POLICIES[0]);
    assert_eq!(agent.choose(board), Some(quick));
    agent.clock(100_000_000);
    let got = agent.limits(board);
    let want = allot(board, 100_000_000, T);
    assert_eq!((got.start_ms, got.time_ms), (want.start_ms, want.time_ms));
    let chosen = agent.choose(board).unwrap();
    assert_eq!(
        exact_after(board, chosen),
        optimum,
        "clocked move {chosen} is not optimal"
    );
}

/// Play uses the gate's clock and policy: fixed limits until the first
/// clock, then variant 3's allocation from the remaining time.
#[test]
fn play_agent_policy() {
    assert_eq!((PLAY_CLOCK_MS, PLAY_POLICY), (51_000, 3));
    let board = line(59);
    let mut agent = play_agent();
    agent.clock(PLAY_CLOCK_MS);
    let policy = othello::search::ClockPolicy::variant(PLAY_POLICY).unwrap();
    let want = othello::search::allot_with(board, PLAY_CLOCK_MS, T, policy);
    let got = agent.limits(board);
    assert_eq!(
        (got.start_ms, got.time_ms, got.early),
        (want.start_ms, want.time_ms, want.early)
    );
}

/// The play clock charges time, rounds the remainder down, never goes
/// below zero, and refunds taken-back moves.
#[test]
fn game_clock_charges() {
    let mut clock = GameClock::new(PLAY_CLOCK_MS);
    assert_eq!(clock.remaining_ms(), 51_000);
    clock.charge(Duration::from_micros(1_500_500));
    assert_eq!(clock.remaining_ms(), 49_499);
    clock.refund(Duration::from_micros(1_500_500));
    assert_eq!(clock.remaining_ms(), 51_000);
    clock.charge(Duration::from_secs(60));
    assert_eq!(clock.remaining_ms(), 0);
    clock.refund(Duration::from_secs(10));
    assert_eq!(clock.remaining_ms(), 1_000);
}
