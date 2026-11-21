use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use othello::board::Board;
use othello::cli::{parse, render};

fn at(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

fn run(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_othello"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn field(text: &str, key: &str) -> String {
    let mut parts = text.split_whitespace();
    while let Some(part) = parts.next() {
        if part == key {
            return parts.next().unwrap().to_string();
        }
    }
    panic!("missing {key} in {text}");
}

#[test]
fn printer_roundtrip() {
    let full = Board {
        me: (1 << 33) - 1,
        opp: !((1 << 33) - 1),
    };
    let cases = [
        (Board::START, true),
        (Board::START, false),
        (
            Board {
                me: at(1, 1),
                opp: at(0, 0) | at(2, 0),
            },
            true,
        ),
        (full, false),
    ];
    for (board, black) in cases {
        let text = render(board, black);
        assert_eq!(parse(&text), Some((board, black)), "{text}");
    }
    assert_eq!(parse(""), None);
    let broken = render(Board::START, true).replacen('X', "?", 1);
    assert_eq!(parse(&broken), None);
}

#[test]
fn perft_three() {
    let output = run(&["perft", "3"], "");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "56");
}

#[test]
fn perft_invalid() {
    let cases: [&[&str]; 4] = [
        &["perft"],
        &["perft", "x"],
        &["perft", "-1"],
        &["perft", "11"],
    ];
    for args in cases {
        let output = run(args, "");
        assert!(!output.status.success(), "{args:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("depth"));
    }
}

#[test]
fn usage_is_shown() {
    let output = run(&[], "");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage"));
}

#[test]
fn eof_exits() {
    let output = run(&["play"], "");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Black to move"));
}

#[test]
fn bench_runs() {
    let output = run(&["bench", "--depth", "2"], "");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("depth 2"), "{text}");
    assert!(text.contains("nodes"), "{text}");

    let output = run(&["bench", "--depth", "x"], "");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("depth"));
}

#[test]
fn illegal_input() {
    let output = run(&["play"], "zz\na1\na9\na0\ni1\nd3x\npass\n");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    for input in ["zz", "a1", "a9", "a0", "i1", "d3x", "pass"] {
        assert!(text.contains(&format!("Illegal move: {input}")), "{text}");
    }
}

#[test]
fn bench_time_controls() {
    let output = run(&["bench", "--depth", "10", "--time-ms", "0"], "");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(field(&text, "depth"), "0", "{text}");
    assert_eq!(field(&text, "nodes"), "0", "{text}");
    assert!(text.contains("nps"), "{text}");
    assert!(text.contains("elapsed"), "{text}");

    let output = run(&["bench", "--depth", "10", "--max-nodes", "1"], "");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(field(&text, "depth"), "0", "{text}");
    assert!(text.contains("elapsed"), "{text}");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release-only")]
fn bench_deadline_smoke() {
    let output = run(&["bench", "--depth", "30", "--time-ms", "200"], "");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    let elapsed: f64 = field(&text, "elapsed")
        .trim_end_matches("ms")
        .parse()
        .unwrap();
    assert!(elapsed <= 250.0, "{text}");
    assert_ne!(field(&text, "depth"), "30", "{text}");
}

#[test]
fn bench_board_position() {
    let board = "785c68e1f14f0d11:0223171e0e3070a0";
    let output = run(
        &[
            "bench",
            "--board",
            board,
            "--endgame-threshold",
            "12",
            "--depth",
            "10",
        ],
        "",
    );
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(field(&text, "depth"), "0", "{text}");
    assert_eq!(field(&text, "score"), "-2200", "{text}");
    assert_eq!(field(&text, "solved"), "1", "{text}");
    assert_eq!(field(&text, "status"), "Complete", "{text}");
}

#[test]
fn bench_board_timeout() {
    let board = "785c68e1f14f0d11:0223171e0e3070a0";
    let output = run(
        &[
            "bench",
            "--board",
            board,
            "--endgame-threshold",
            "12",
            "--depth",
            "10",
            "--time-ms",
            "0",
        ],
        "",
    );
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(field(&text, "depth"), "0", "{text}");
    assert_eq!(field(&text, "solved"), "0", "{text}");
    let status = field(&text, "status");
    assert!(status == "Fallback" || status == "Partial", "{text}");
}

#[test]
fn bench_board_invalid() {
    for board in ["xyz", "1:1", "0:0"] {
        let output = run(&["bench", "--board", board], "");
        assert!(!output.status.success(), "{board}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("invalid board"),
            "{board}"
        );
    }
    let output = run(&["bench", "--board", "200:5"], "");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("legal"));
    let output = run(&["bench", "--endgame-threshold", "x"], "");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid threshold"));
}

#[test]
fn play_opponent_is_search() {
    let output = run(
        &["play"],
        "d3
",
    );
    assert!(output.status.success());
    // A legal-reply smoke test: under the play clock (#167) the reply's
    // timing is not fixed, so only exit and output are checked.
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.matches("Black to move").count() >= 2 || text.contains("Game over"),
        "{text}"
    );
}

#[test]
#[ignore = "release-only"]
fn play_complete_game() {
    let start = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_othello"))
        .arg("play")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let output = BufReader::new(child.stdout.take().unwrap());
    let mut moves = 0;
    let mut passes = 0;
    for line in output.lines() {
        let line = line.unwrap();
        if line.starts_with("Game over.") {
            let discs: Vec<u32> = line
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|part| part.parse().ok())
                .collect();
            assert_eq!(discs.len(), 2, "{line}");
            assert!(discs[0] + discs[1] <= 64, "{line}");
            break;
        }
        if line.contains("Black passes") {
            passes += 1;
        }
        if line.starts_with("Black to move.") {
            let legal = line.split("legal:").nth(1).unwrap_or("").trim();
            if !legal.is_empty() {
                writeln!(input, "{}", legal.split_whitespace().next().unwrap()).unwrap();
                moves += 1;
            }
        }
    }
    assert!(moves > 0);
    println!("black passes observed: {passes}");
    assert!(start.elapsed().as_secs() < 120, "game timed out");
    assert!(child.wait().unwrap().success());
}

/// Greedy play that logs its calls in order: `Some(ms)` for a clock
/// update, `None` for a move.
struct Recorder(othello::agents::GreedyAgent, Vec<Option<u64>>);

impl othello::agents::Agent for Recorder {
    fn choose(&mut self, board: othello::board::Board) -> Option<u8> {
        self.1.push(None);
        self.0.choose(board)
    }

    fn clock(&mut self, remaining_ms: u64) {
        self.1.push(Some(remaining_ms));
    }
}

fn recorder() -> Recorder {
    Recorder(othello::agents::greedy_agent(), Vec::new())
}

/// The engine turn gives the agent the remaining clock before each move
/// and charges the move: 51,000 first, then 48,500 after a 2.5 s move.
#[test]
fn play_turn_clock() {
    use othello::agents::{GameClock, PLAY_CLOCK_MS};
    use othello::board::{Board, legal_moves, play};
    use othello::cli::engine_turn;
    let mut agent = recorder();
    let mut clock = GameClock::new(PLAY_CLOCK_MS);
    let step = &mut |_: Instant| Duration::from_millis(2500);
    let first = engine_turn(&mut agent, &mut clock, Board::START, step).unwrap();
    assert_eq!(clock.remaining_ms(), PLAY_CLOCK_MS - 2500);
    let mut board = play(Board::START, first);
    let reply = legal_moves(board.me, board.opp).trailing_zeros() as u8;
    board = play(board, reply);
    engine_turn(&mut agent, &mut clock, board, step).unwrap();
    // Each clock update comes before the move it serves.
    let calls = [Some(PLAY_CLOCK_MS), None, Some(PLAY_CLOCK_MS - 2500), None];
    assert_eq!(agent.1, calls);
    assert_eq!(clock.remaining_ms(), PLAY_CLOCK_MS - 5000);
}

/// A pass calls no agent and charges nothing.
#[test]
fn play_turn_pass() {
    use othello::agents::{GameClock, PLAY_CLOCK_MS};
    use othello::cli::engine_turn;
    let stuck = othello::board::Board { me: 1 << 1, opp: 1 };
    let mut agent = recorder();
    let mut clock = GameClock::new(PLAY_CLOCK_MS);
    let step = &mut |_: Instant| Duration::from_millis(2500);
    assert_eq!(engine_turn(&mut agent, &mut clock, stuck, step), None);
    assert!(agent.1.is_empty(), "neither clock nor choose is called");
    assert_eq!(clock.remaining_ms(), PLAY_CLOCK_MS);
}

/// An exhausted clock does not forfeit: the agent is told 0 and still
/// moves, and the clock stays at 0.
#[test]
fn play_turn_overrun() {
    use othello::agents::{GameClock, PLAY_CLOCK_MS};
    use othello::board::{Board, legal_moves};
    use othello::cli::engine_turn;
    let mut agent = recorder();
    let mut clock = GameClock::new(PLAY_CLOCK_MS);
    clock.charge(Duration::from_secs(60));
    let step = &mut |_: Instant| Duration::from_millis(2500);
    let square = engine_turn(&mut agent, &mut clock, Board::START, step).unwrap();
    assert_ne!(
        legal_moves(Board::START.me, Board::START.opp) & (1 << square),
        0
    );
    assert_eq!(agent.1, [Some(0), None]);
    assert_eq!(clock.remaining_ms(), 0);
}

fn forbidden(_: othello::board::Board) -> i32 {
    panic!("a forced move must not search")
}

/// The play agent's forced shortcut through the engine turn: a lone legal
/// move is played without evaluating, and the call is still charged.
#[test]
fn play_turn_forced() {
    use othello::agents::{GameClock, PLAY_CLOCK_MS, PLAY_POLICY, clock_agent};
    use othello::board::{Board, empty_count, is_terminal, legal_moves, pass, play};
    use othello::cli::engine_turn;
    use othello::search::{ClockPolicy, SearchConfig, SearchLimits};
    let (board, only) = (0..10_000u64)
        .find_map(|seed| {
            let mut rng = othello::rng::Rng::new(seed);
            let mut board = Board::START;
            while !is_terminal(board) {
                let moves = legal_moves(board.me, board.opp);
                if moves.count_ones() == 1 && empty_count(board) > 20 {
                    return Some((board, moves.trailing_zeros() as u8));
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
            None
        })
        .expect("a lone midgame move");
    let config = SearchConfig {
        eval: forbidden,
        ..SearchConfig::standard()
    };
    let policy = ClockPolicy::variant(PLAY_POLICY).unwrap();
    let mut agent = clock_agent(config, SearchLimits::new(64, 3000, 0), policy);
    let mut clock = GameClock::new(PLAY_CLOCK_MS);
    let step = &mut |_: Instant| Duration::from_millis(7);
    assert_eq!(engine_turn(&mut agent, &mut clock, board, step), Some(only));
    assert_eq!(clock.remaining_ms(), PLAY_CLOCK_MS - 7);
}
