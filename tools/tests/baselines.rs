use std::process::Command;

use othello::board::Board;
use othello_tools::baselines::{PHASE2_DEPTH, eval_phase2, phase2_config};

fn at(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

#[test]
fn eval_pins() {
    assert_eq!(eval_phase2(Board::START), 0);
    let cases = [
        (at(0, 0), 200),
        (at(1, 0), 80),
        (at(1, 1), 50),
        (at(2, 0), 110),
        (at(3, 0), 105),
        (at(2, 1), 98),
        (at(3, 1), 98),
        (at(2, 2), 99),
    ];
    for (me, expected) in cases {
        assert_eq!(eval_phase2(Board { me, opp: 0 }), expected, "{me:#x}");
    }
    assert_eq!(
        eval_phase2(Board {
            me: u64::MAX,
            opp: 0
        }),
        6400
    );
}

#[test]
fn config_pins() {
    let config = phase2_config();
    assert_eq!((config.eval)(Board::START), 0);
    assert_eq!(config.endgame_threshold, 12);
    assert_eq!(PHASE2_DEPTH, 4);
}

fn tools(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_othello-tools"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn match_smoke() {
    let output = tools(&[
        "match", "--a", "greedy", "--b", "greedy", "--pairs", "2", "--seed", "1",
    ]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("pairs 2"), "{text}");
    assert!(text.contains("mean 0.5000"), "{text}");
}

#[test]
fn phase2_selectable() {
    let output = tools(&[
        "match", "--a", "phase2", "--b", "greedy", "--pairs", "1", "--seed", "1",
    ]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("pairs 1"));
}

#[test]
fn unknown_agent_rejected() {
    let output = tools(&["match", "--a", "nope", "--b", "greedy"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown agent"));
}
