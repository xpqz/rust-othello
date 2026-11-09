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
    assert!(text.contains("a greedy"), "{text}");
    assert!(text.contains("b greedy"), "{text}");
    assert!(
        text.contains("seed 1 resamples 10000 evaluator-only"),
        "{text}"
    );
}

#[test]
fn phase2_selectable() {
    let output = tools(&[
        "match", "--a", "phase2", "--b", "greedy", "--pairs", "1", "--seed", "1",
    ]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("pairs 1"), "{text}");
    assert!(
        text.contains("phase2 (eval_phase2, depth 4, time none, nodes 0, threshold 12)"),
        "{text}"
    );
}

#[test]
fn random_match_reproducible() {
    let args = [
        "match", "--a", "random", "--b", "greedy", "--pairs", "10", "--seed", "1",
    ];
    let first = tools(&args);
    let second = tools(&args);
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
    let other = tools(&[
        "match", "--a", "random", "--b", "greedy", "--pairs", "10", "--seed", "2",
    ]);
    let line = |output: &std::process::Output| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap()
            .to_string()
    };
    assert_ne!(line(&first), line(&other));
}

#[test]
fn ab_selectable() {
    let output = tools(&[
        "match", "--a", "ab:2", "--b", "greedy", "--pairs", "1", "--seed", "1",
    ]);
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("ab:2 (evaluate, depth 2, time none, nodes 0, threshold 14)")
    );
}

#[test]
fn ab_invalid() {
    for name in ["ab:x", "ab:"] {
        let output = tools(&["match", "--a", name, "--b", "greedy"]);
        assert!(!output.status.success(), "{name}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("unknown agent"));
    }
}

#[test]
fn frozen_source() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/baselines.rs");
    let source = std::fs::read_to_string(path).unwrap();
    assert!(!source.contains("evaluate"));
}

#[test]
fn unknown_agent_rejected() {
    let output = tools(&["match", "--a", "nope", "--b", "greedy"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown agent"));
}

#[test]
fn degenerate_output() {
    let output = tools(&[
        "match", "--a", "greedy", "--b", "greedy", "--pairs", "10", "--seed", "1",
    ]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("uninformative"), "{text}");
    assert!(text.contains("elo none"), "{text}");
    let zero = tools(&[
        "match", "--a", "greedy", "--b", "greedy", "--pairs", "0", "--seed", "1",
    ]);
    assert!(zero.status.success());
    assert!(String::from_utf8_lossy(&zero.stdout).contains("insufficient"));
}

#[test]
fn pairs_limit() {
    let path = std::env::temp_dir().join(format!("othello_openings_{}.txt", std::process::id()));
    std::fs::write(&path, "c4 e3 f2 c5\n").unwrap();
    let output = tools(&[
        "match",
        "--a",
        "greedy",
        "--b",
        "greedy",
        "--pairs",
        "2",
        "--openings",
        path.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exceeds"));
}

#[test]
fn log_seeds() {
    let output = tools(&[
        "match", "--a", "random", "--b", "random", "--pairs", "1", "--seed", "7",
    ]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("run "))
        .collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].starts_with("run 7 pair 0 game 0"), "{text}");
    assert!(lines[1].starts_with("run 7 pair 0 game 1"), "{text}");
    let seeds = |line: &str| {
        line.split("seeds ")
            .nth(1)
            .unwrap()
            .split(" key")
            .next()
            .unwrap()
            .to_string()
    };
    assert_ne!(seeds(lines[0]), seeds(lines[1]));
}

#[test]
fn timed_and_node_agents() {
    let output = tools(&[
        "match",
        "--a",
        "ab:2:1000",
        "--b",
        "greedy",
        "--pairs",
        "1",
        "--seed",
        "1",
    ]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("ab:2:1000 (evaluate, depth 64, time 1000"),
        "{text}"
    );

    let output = tools(&[
        "match",
        "--a",
        "phase2:1000",
        "--b",
        "greedy",
        "--pairs",
        "1",
        "--seed",
        "1",
    ]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("phase2:1000 (eval_phase2, depth 64, time 1000"),
        "{text}"
    );
}

#[test]
fn node_limited_match() {
    let unlimited = tools(&[
        "match", "--a", "phase2", "--b", "greedy", "--pairs", "2", "--seed", "1",
    ]);
    let args = [
        "match", "--a", "phase2", "--b", "greedy", "--pairs", "2", "--seed", "1", "--nodes", "100",
    ];
    let first = tools(&args);
    let second = tools(&args);
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
    assert!(String::from_utf8_lossy(&first.stdout).contains("nodes 100"));
    let logs = |output: &std::process::Output| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| line.starts_with("run "))
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_ne!(logs(&first), logs(&unlimited));
}

#[test]
fn malformed_agent_suffixes() {
    for name in ["ab:2:x", "ab:2:", "phase2:x"] {
        let output = tools(&["match", "--a", name, "--b", "greedy"]);
        assert!(!output.status.success(), "{name}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("unknown agent"));
    }
}

#[test]
fn node_rule_records_limits() {
    let output = tools(&[
        "match", "--a", "ab:2", "--b", "greedy", "--pairs", "1", "--nodes", "100",
    ]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("ab:2 (evaluate, depth 64, time none, nodes 100, threshold 14)"),
        "{text}"
    );

    let output = tools(&[
        "match",
        "--a",
        "ab:2:1000",
        "--b",
        "greedy",
        "--nodes",
        "100",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflicts"));
}

#[test]
fn pc_agents_parse() {
    let output = tools(&[
        "match", "--a", "pc:150", "--b", "pc:off", "--pairs", "1", "--nodes", "100",
    ]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("pc:150 (evaluate, depth 64"), "{text}");
    assert!(text.contains("probcut t=150"), "{text}");
    assert!(text.contains("pc:off (evaluate, depth 64"), "{text}");
    assert!(text.contains("probcut off"), "{text}");

    for name in ["pc", "pc:30", "pc:150:x", "pc:off:1:2"] {
        let output = tools(&["match", "--a", name, "--b", "greedy", "--pairs", "1"]);
        assert!(!output.status.success(), "{name}");
    }
    for name in ["pc:150:10", "pc:off:10"] {
        let output = tools(&["match", "--a", name, "--b", "greedy", "--pairs", "1"]);
        assert!(output.status.success(), "{name}");
    }
}

#[test]
fn pc_off_uses_the_full_node_budget() {
    use othello::board::{Board, empty_count, legal_moves};
    use othello::search::{SearchConfig, SearchLimits, search_limited};
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/corpus.txt"))
        .unwrap();
    let config = SearchConfig {
        probcut: None,
        ..SearchConfig::standard()
    };
    let found = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let me = u64::from_str_radix(parts.next().unwrap(), 16).unwrap();
            let opp = u64::from_str_radix(parts.next().unwrap(), 16).unwrap();
            let board = Board { me, opp };
            ((20..=40).contains(&empty_count(board)) && legal_moves(board.me, board.opp) != 0)
                .then_some(board)
        })
        .find(|&board| {
            search_limited(board, SearchLimits::new(64, u64::MAX, 1000), &config).nodes >= 900
        });
    assert!(
        found.is_some(),
        "no midgame corpus position used the full budget"
    );
}
