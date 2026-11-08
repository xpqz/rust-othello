use std::io::Write;
use std::process::{Command, Output, Stdio};

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
}

#[test]
fn perft_three() {
    let output = run(&["perft", "3"], "");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "56");
}

#[test]
fn perft_invalid() {
    for depth in ["x", "11"] {
        let output = run(&["perft", depth], "");
        assert!(!output.status.success(), "{depth}");
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
}

#[test]
fn illegal_input() {
    let output = run(&["play"], "zz\na1\n");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Illegal move: zz"), "{text}");
    assert!(text.contains("Illegal move: a1"), "{text}");
}

#[test]
fn scripted_game() {
    let mut board = Board::START;
    let mut black = true;
    let mut agent = greedy_agent();
    let mut human = Vec::new();
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
    assert!(
        text.contains(&format!(
            "Game over. Black {black_discs}, White {white_discs}."
        )),
        "{text}"
    );
}
