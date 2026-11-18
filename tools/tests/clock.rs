use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use othello::agents::{Agent, GreedyAgent, greedy_agent};
use othello::board::Board;
use othello::search::MAX_CLOCK_MS;
use othello_tools::agents::{clock, config, parse_name};
use othello_tools::harness::{Clocked, clocked_points, play_clocked};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// Greedy play that records the clock it is given and counts its moves.
struct Recorder {
    inner: GreedyAgent,
    clocks: Vec<u64>,
    moves: usize,
}

impl Agent for Recorder {
    fn choose(&mut self, board: Board) -> Option<u8> {
        self.moves += 1;
        self.inner.choose(board)
    }

    fn clock(&mut self, remaining_ms: u64) {
        self.clocks.push(remaining_ms);
    }
}

fn recorder() -> Recorder {
    Recorder {
        inner: greedy_agent(),
        clocks: Vec::new(),
        moves: 0,
    }
}

/// Plays greedy against greedy from the start, charging `step` per call.
fn game(clocks: [Option<Duration>; 2], step: Duration) -> (Clocked, Recorder, Recorder, usize) {
    let (mut black, mut white) = (recorder(), recorder());
    let mut calls = 0;
    let mut elapsed = |_: Instant| {
        calls += 1;
        step
    };
    let game = play_clocked(&[], &mut black, &mut white, clocks, &mut elapsed).unwrap();
    (game, black, white, calls)
}

#[test]
fn clock_debits() {
    let step = Duration::from_micros(1500);
    let (game, black, white, calls) = game([None, None], step);
    assert_eq!(
        calls,
        black.moves + white.moves,
        "every choose is charged once"
    );
    assert_eq!(game.used[0], step * black.moves as u32);
    assert_eq!(game.used[1], step * white.moves as u32);
    assert_eq!(game.flag, None);
    assert!(black.clocks.is_empty() && white.clocks.is_empty());
}

/// Black's third move takes its charged time to 12 ms, over a 10 ms clock:
/// the move is not played, black loses.
#[test]
fn black_flags() {
    let (game, black, _, _) = game([Some(ms(10)), None], ms(4));
    assert_eq!(game.flag, Some(0));
    assert_eq!(game.result.plies, 4);
    assert_eq!(black.moves, 3);
    assert_eq!((game.used[0], game.used[1]), (ms(12), ms(8)));
    assert_eq!(clocked_points(&game), 0.0);
}

#[test]
fn white_flags() {
    let (game, _, white, _) = game([None, Some(ms(10))], ms(4));
    assert_eq!(game.flag, Some(1));
    assert_eq!(game.result.plies, 5);
    assert_eq!(white.moves, 3);
    assert_eq!(clocked_points(&game), 1.0);
}

/// Charged time equal to the clock is not a flag; a nanosecond over is.
#[test]
fn clock_boundary() {
    let (exact, _, _, _) = game([Some(ms(12)), None], ms(4));
    assert_eq!(exact.flag, Some(0));
    assert_eq!(
        exact.result.plies, 6,
        "flags on the fourth move, not the third"
    );
    let (over, _, _, _) = game([Some(ms(12) - Duration::from_nanos(1)), None], ms(4));
    assert_eq!(over.flag, Some(0));
    assert_eq!(over.result.plies, 4);
}

/// The clocked side gets the remaining whole milliseconds before each move.
#[test]
fn clock_remaining() {
    let (game, black, white, _) = game([Some(ms(10)), None], Duration::from_micros(2500));
    assert_eq!(black.clocks, [10, 7, 5, 2, 0]);
    assert_eq!(game.flag, Some(0));
    assert_eq!(game.result.plies, 8);
    assert!(white.clocks.is_empty());
}

#[test]
fn unclocked_never_flags() {
    let (game, black, white, _) = game([None, None], Duration::from_secs(3600));
    assert_eq!(game.flag, None);
    assert_eq!(game.used[0], Duration::from_secs(3600) * black.moves as u32);
    assert_eq!(game.used[1], Duration::from_secs(3600) * white.moves as u32);
}

/// Opening moves and passes call no agent and are not charged.
#[test]
fn opening_free() {
    let (mut black, mut white) = (recorder(), recorder());
    let mut calls = 0;
    let mut elapsed = |_: Instant| {
        calls += 1;
        ms(1)
    };
    let opening = [19, 18];
    let game = play_clocked(&opening, &mut black, &mut white, [None, None], &mut elapsed).unwrap();
    assert_eq!(calls, black.moves + white.moves);
    let passes = game.result.passes as usize;
    assert_eq!(game.result.plies as usize, opening.len() + calls + passes);
}

#[test]
fn clock_grammar() {
    for (name, want) in [
        ("pc:80:c5000", Some(5000)),
        ("phase2:c7", Some(7)),
        ("classic:c9", Some(9)),
        ("ab:2:c10", Some(10)),
        ("greedy:c10", Some(10)),
        ("random:c10", Some(10)),
        ("pc:80:c5000+book", Some(5000)),
        ("pc:80:3000", None),
        ("pc:80:3000+book", None),
        ("greedy", None),
    ] {
        assert_eq!(clock(name), Ok(want), "{name}");
    }
    let top = format!("pc:80:c{MAX_CLOCK_MS}");
    assert_eq!(clock(&top), Ok(Some(MAX_CLOCK_MS)));
    let (kind, depth, _) = parse_name("pc:80:c5000").unwrap();
    assert_eq!((kind, depth), ("pc", 64));
    let over = format!("pc:80:c{}", MAX_CLOCK_MS + 1);
    for bad in [
        "pc:80:c0",
        over.as_str(),
        "pc:80:cx",
        "pc:80:c",
        "classic:d4:c100",
        "random+book",
        "greedy+book",
        "greedy:c10+book",
        "pc:80:3000+bok",
    ] {
        assert!(
            parse_name(bad).is_err() || clock(bad).is_err(),
            "{bad} accepted"
        );
    }
    assert!(
        config("ab:2:c100", 20000, true).is_err(),
        "clock with --nodes"
    );
    assert!(config("pc:80:3000+book", 0, true).unwrap().0.book);
    assert!(config("pc:80:c5000+book", 0, true).unwrap().0.book);
    assert!(!config("pc:80:c5000", 0, true).unwrap().0.book);
}

fn temp(label: &str) -> PathBuf {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "othello_clock_{}_{}_{}",
        std::process::id(),
        id,
        label
    ))
}

/// `othello-tools` with a fixed load and a fixed charge per `choose`.
fn tools(load: &str, elapsed_ms: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_othello-tools"))
        .env("OTHELLO_TOOLS_LOAD", load)
        .env("OTHELLO_TOOLS_ELAPSED_MS", elapsed_ms)
        .args(args)
        .output()
        .unwrap()
}

fn greedy_match(a: &str, out: &Path, max_load: &str) -> Vec<String> {
    let out = out.to_str().unwrap();
    [
        "match", "--a", a, "--b", "greedy", "--pairs", "2", "--seed", "7",
    ]
    .into_iter()
    .chain(["--jobs", "1", "--max-load", max_load, "--out", out])
    .map(String::from)
    .collect()
}

fn run(load: &str, elapsed_ms: &str, args: &[String]) -> Output {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    tools(load, elapsed_ms, &refs)
}

fn payloads(path: &Path) -> Vec<String> {
    let text = fs::read_to_string(path).unwrap();
    let mut lines: Vec<String> = text
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once('\t').map(|(_, p)| p.to_string()))
        .collect();
    lines.sort();
    lines
}

fn value<'a>(line: &'a str, key: &str) -> &'a str {
    let mut parts = line.split_whitespace();
    while let Some(part) = parts.next() {
        if part == key {
            return parts.next().unwrap();
        }
    }
    panic!("no {key} in {line}")
}

fn report(path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_othello-tools"))
        .args(["report", path.to_str().unwrap()])
        .output()
        .unwrap()
}

#[test]
fn journal_fields() {
    let path = temp("fields");
    let done = run("0", "3", &greedy_match("greedy:c100000", &path, "1000"));
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let header = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string();
    assert_eq!(value(&header, "control"), "clock");
    let lines = payloads(&path);
    assert_eq!(lines.len(), 4);
    for line in &lines {
        assert_eq!(value(line, "flag"), "none");
        for side in ["black_ms", "white_ms"] {
            let charged: u64 = value(line, side).parse().unwrap();
            assert!(charged > 0 && charged.is_multiple_of(3), "{side} {charged}");
        }
    }
    fs::remove_file(path).unwrap();
}

/// `greedy:c10` at 4 ms a move flags on its third move in every game; the
/// report scores it 0 and states each side's time and flags.
#[test]
fn flags_scored() {
    let path = temp("flags");
    let done = run("0", "4", &greedy_match("greedy:c10", &path, "1000"));
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let lines = payloads(&path);
    let flags: Vec<&str> = lines.iter().map(|l| value(l, "flag")).collect();
    assert_eq!(flags.iter().filter(|f| **f == "black").count(), 2);
    assert_eq!(flags.iter().filter(|f| **f == "white").count(), 2);
    let text = String::from_utf8_lossy(&report(&path).stdout).to_string();
    assert!(text.contains("mean 0.0000"), "{text}");
    assert!(text.contains("time a 12 b 10 flags a 4 b 0"), "{text}");
    fs::remove_file(path).unwrap();
}

/// A flagged pair above the load limit is void like any other.
#[test]
fn flagged_void() {
    let path = temp("void");
    let done = run("9", "4", &greedy_match("greedy:c10", &path, "7"));
    assert!(!done.status.success());
    let stderr = String::from_utf8_lossy(&done.stderr);
    assert!(stderr.contains("void after ten passes"), "{stderr}");
    assert!(stderr.contains("void pair 0 load 9.000"), "{stderr}");
    assert!(payloads(&path).is_empty());
    fs::remove_file(path).unwrap();
}

/// One load value: the journal's three decimals decide, live and on resume,
/// for the measured load and the configured limit alike.
#[test]
fn load_rounded() {
    let path = temp("load");
    let args = greedy_match("greedy:c100000", &path, "6");
    assert!(run("6.0004", "1", &args).status.success());
    assert!(payloads(&path).iter().all(|l| value(l, "load") == "6.000"));
    let before = fs::read(&path).unwrap();
    assert!(run("6.0004", "1", &args).status.success());
    assert_eq!(fs::read(&path).unwrap(), before, "resume keeps the pairs");
    assert!(report(&path).status.success());
    fs::remove_file(&path).unwrap();
    assert!(!run("6.0006", "1", &args).status.success(), "6.001 > 6");
    fs::remove_file(&path).unwrap();
    let limit = greedy_match("greedy:c100000", &path, "6.0004");
    assert!(run("6.0004", "1", &limit).status.success());
    let header = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string();
    assert_eq!(value(&header, "max_load"), "6.000");
    fs::remove_file(path).unwrap();
}

#[test]
fn clock_header_mismatch() {
    let path = temp("mismatch");
    assert!(
        run("0", "1", &greedy_match("greedy:c100000", &path, "1000"))
            .status
            .success()
    );
    let changed = run("0", "1", &greedy_match("greedy:c90000", &path, "1000"));
    assert!(!changed.status.success());
    assert!(String::from_utf8_lossy(&changed.stderr).contains("mismatch"));
    fs::remove_file(path).unwrap();
}

#[test]
fn clock_cli_boundary() {
    let path = temp("boundary");
    let top = format!("greedy:c{MAX_CLOCK_MS}");
    assert!(
        run("0", "1", &greedy_match(&top, &path, "1000"))
            .status
            .success()
    );
    fs::remove_file(&path).unwrap();
    let over = format!("greedy:c{}", MAX_CLOCK_MS + 1);
    assert!(
        !run("0", "1", &greedy_match(&over, &path, "1000"))
            .status
            .success()
    );
    let nodes: Vec<String> = greedy_match("ab:2:c100", &path, "1000")
        .into_iter()
        .chain(["--nodes".into(), "2000".into()])
        .collect();
    assert!(
        !run("0", "1", &nodes).status.success(),
        "clock with --nodes"
    );
}

/// A clocked match with injected time and load, killed and resumed, ends
/// with the same journal records and report as an uninterrupted run.
#[test]
fn clock_kill_resume() {
    let (reference, resumed) = (temp("reference"), temp("resumed"));
    let args = |out: &Path| -> Vec<String> {
        let out = out.to_str().unwrap();
        ["match", "--a", "ab:4", "--b", "greedy:c40", "--pairs", "6"]
            .into_iter()
            .chain(["--nodes", "20000", "--seed", "7", "--jobs", "2"])
            .chain(["--max-load", "1000", "--out", out])
            .map(String::from)
            .collect()
    };
    assert!(run("0", "3", &args(&reference)).status.success());
    let mut child = Command::new(env!("CARGO_BIN_EXE_othello-tools"))
        .env("OTHELLO_TOOLS_LOAD", "0")
        .env("OTHELLO_TOOLS_ELAPSED_MS", "3")
        .args(args(&resumed))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        let lines = fs::read_to_string(&resumed).map_or(0, |text| text.lines().count());
        if lines >= 3 {
            child.kill().unwrap();
            break;
        }
        if child.try_wait().unwrap().is_some() || start.elapsed() > Duration::from_secs(30) {
            panic!("run finished before it could be interrupted");
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(child.wait().unwrap().signal(), Some(9));
    assert!(fs::read_to_string(&resumed).unwrap().lines().count() < 13);
    assert!(run("0", "3", &args(&resumed)).status.success());
    assert_eq!(payloads(&reference), payloads(&resumed));
    assert_eq!(report(&reference).stdout, report(&resumed).stdout);
    assert!(
        payloads(&reference)
            .iter()
            .any(|l| value(l, "flag") != "none")
    );
    fs::remove_file(reference).unwrap();
    fs::remove_file(resumed).unwrap();
}

/// An engine with a game clock allocates from it, so a 5 s clock lasts the
/// game; fixed per-move limits of 5 s would flag at once.
#[test]
fn engine_clock_smoke() {
    let path = temp("engine");
    let out = path.to_str().unwrap();
    let done = Command::new(env!("CARGO_BIN_EXE_othello-tools"))
        .env("OTHELLO_TOOLS_LOAD", "0")
        .args([
            "match",
            "--a",
            "pc:80:c5000",
            "--b",
            "greedy",
            "--pairs",
            "1",
        ])
        .args([
            "--seed",
            "7",
            "--jobs",
            "1",
            "--max-load",
            "1000",
            "--out",
            out,
        ])
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    for line in payloads(&path) {
        assert_eq!(value(&line, "flag"), "none");
    }
    fs::remove_file(path).unwrap();
}
