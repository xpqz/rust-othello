use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::Instant;

use othello::agents::{Agent, greedy_agent};
use othello::board::{Board, is_terminal, legal_moves, pass, play};
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

fn notation(square: u8) -> String {
    let file = (b'a' + square % 8) as char;
    let rank = (b'1' + square / 8) as char;
    format!("{file}{rank}")
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
fn scripted_game() {
    let mut board = Board::START;
    let mut black = true;
    let mut agent = greedy_agent();
    let mut human = Vec::new();
    let mut white_passes = 0;
    while !is_terminal(board) {
        let moves = legal_moves(board.me, board.opp);
        if black {
            if moves == 0 {
                human.push("pass".to_string());
                board = pass(board);
            } else {
                let square = moves.trailing_zeros() as u8;
                human.push(notation(square));
                board = play(board, square);
            }
        } else if moves == 0 {
            white_passes += 1;
            board = pass(board);
        } else {
            let square = agent.choose(board).unwrap();
            board = play(board, square);
        }
        black = !black;
    }
    let (black_discs, white_discs) = if black {
        (board.me.count_ones(), board.opp.count_ones())
    } else {
        (board.opp.count_ones(), board.me.count_ones())
    };
    let input = format!("{}\n", human.join("\n"));
    let output = run(&["play"], &input);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(white_passes > 0);
    assert!(text.contains("White passes"), "{text}");
    assert!(
        text.contains(&format!(
            "Game over. Black {black_discs}, White {white_discs}."
        )),
        "{text}"
    );
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
    let start = Instant::now();
    let output = run(
        &["play"],
        "d3
",
    );
    assert!(output.status.success());
    assert!(
        start.elapsed().as_millis() >= 500,
        "White responded too quickly to be the search agent"
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("Black to move") || text.contains("Game over"),
        "{text}"
    );
}
