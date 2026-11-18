use std::collections::BTreeMap;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use othello::agents::{Agent, greedy_agent, random_agent, search_agent};
use othello::board::{Board, disc_count, legal_moves, pass, play};
use othello::book::book_move;
use othello::search::{MAX_CLOCK_MS, SearchConfig, SearchLimits, allot};
use othello_tools::agents::{clock, config, parse_name, search};
use othello_tools::harness::{Clocked, clocked_points, play_clocked};
use othello_tools::journal::read;
use othello_tools::positions;

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// Wraps an agent, recording the clocks it is given and counting its moves.
struct Recorder<A: Agent> {
    inner: A,
    clocks: Vec<u64>,
    moves: usize,
    first: Option<u8>,
}

impl<A: Agent> Agent for Recorder<A> {
    fn choose(&mut self, board: Board) -> Option<u8> {
        self.moves += 1;
        let square = self.inner.choose(board);
        self.first = self.first.or(square);
        square
    }

    fn clock(&mut self, remaining_ms: u64) {
        self.clocks.push(remaining_ms);
    }
}

fn record<A: Agent>(inner: A) -> Recorder<A> {
    Recorder {
        inner,
        clocks: Vec::new(),
        moves: 0,
        first: None,
    }
}

type Greedy = Recorder<othello::agents::GreedyAgent>;

/// Greedy against greedy from `opening`, charging `step` per call.
fn game_from(
    opening: &[u8],
    clocks: [Option<Duration>; 2],
    step: Duration,
) -> (Clocked, Greedy, Greedy, usize) {
    let (mut black, mut white) = (record(greedy_agent()), record(greedy_agent()));
    let mut calls = 0;
    let mut elapsed = |_: Instant| {
        calls += 1;
        step
    };
    let game = play_clocked(opening, &mut black, &mut white, clocks, &mut elapsed).unwrap();
    (game, black, white, calls)
}

fn game(clocks: [Option<Duration>; 2], step: Duration) -> (Clocked, Greedy, Greedy, usize) {
    game_from(&[], clocks, step)
}

/// Black's centidisc lead after `opening` and `plies` greedy plies.
fn greedy_lead(opening: &[u8], plies: usize) -> i32 {
    let mut board = Board::START;
    let mut black = true;
    for &square in opening {
        board = play(board, square);
        black = !black;
    }
    let mut agent = greedy_agent();
    for _ in 0..plies {
        board = if legal_moves(board.me, board.opp) == 0 {
            pass(board)
        } else {
            play(board, agent.choose(board).unwrap())
        };
        black = !black;
    }
    let lead = disc_count(board.me) as i32 - disc_count(board.opp) as i32;
    100 * if black { lead } else { -lead }
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
    assert_eq!(game.result.score, greedy_lead(&[], 4));
    assert_eq!(clocked_points(&game), 0.0);
}

#[test]
fn white_flags() {
    let (game, _, white, _) = game([None, Some(ms(10))], ms(4));
    assert_eq!(game.flag, Some(1));
    assert_eq!(game.result.plies, 5);
    assert_eq!(white.moves, 3);
    assert_eq!(game.result.score, greedy_lead(&[], 5));
    assert_eq!(clocked_points(&game), 1.0);
}

/// The stopped board's score is the disc lead before the discarded move,
/// kept apart from the forfeit: here the flagging side is ahead and still
/// loses.
#[test]
fn stopped_score() {
    let step = ms(4);
    let k = (2..30)
        .find(|&k| {
            let plies = 2 * (k - 1);
            greedy_lead(&[], plies) > 0 && greedy_lead(&[], plies + 1) != greedy_lead(&[], plies)
        })
        .expect("black ahead with the move at some flag point");
    let clock = step * (k as u32 - 1);
    let (game, black, _, _) = game([Some(clock), None], step);
    assert_eq!(game.flag, Some(0));
    assert_eq!(black.moves, k);
    let plies = 2 * (k - 1);
    assert_eq!(game.result.plies as usize, plies);
    assert_eq!(game.result.score, greedy_lead(&[], plies));
    assert!(game.result.score > 0);
    assert_eq!(clocked_points(&game), 0.0);
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

/// Opening moves call no agent and are not charged.
#[test]
fn opening_free() {
    let opening = [19, 18];
    let (game, black, white, calls) = game_from(&opening, [None, None], ms(1));
    assert_eq!(calls, black.moves + white.moves);
    let passes = game.result.passes as usize;
    assert_eq!(game.result.plies as usize, opening.len() + calls + passes);
}

/// Passes call no agent and are not charged: a seeded random game that
/// passes at least once.
#[test]
fn passes_free() {
    for seed in 0..1000 {
        let (mut black, mut white) = (record(random_agent(seed)), record(random_agent(!seed)));
        let mut calls = 0;
        let mut elapsed = |_: Instant| {
            calls += 1;
            ms(1)
        };
        let game = play_clocked(&[], &mut black, &mut white, [None, None], &mut elapsed).unwrap();
        if game.result.passes == 0 {
            continue;
        }
        assert_eq!(calls, black.moves + white.moves);
        assert_eq!(game.used[0] + game.used[1], ms(calls as u64));
        assert_eq!(game.result.plies, calls as u32 + game.result.passes);
        return;
    }
    panic!("no game with a pass in 1,000 seeds");
}

/// A book hit is a `choose` call like any other and is charged in full.
#[test]
fn book_charged() {
    let booked = SearchConfig {
        book: true,
        ..SearchConfig::standard()
    };
    let mut black = record(search_agent(booked, SearchLimits::depth(1)));
    let mut white = record(greedy_agent());
    let mut elapsed = |_: Instant| ms(7);
    let game = play_clocked(&[], &mut black, &mut white, [None, None], &mut elapsed).unwrap();
    assert_eq!(black.first, Some(book_move(Board::START).unwrap().0));
    assert_eq!(game.used[0], ms(7) * black.moves as u32);
}

#[test]
fn clock_grammar() {
    for (name, want) in [
        ("pc:80:c5000", Some(5000)),
        ("phase2:c7", Some(7)),
        ("classic:c9", Some(9)),
        ("ab:2:c10", Some(10)),
        ("pc:80:c5000+book", Some(5000)),
        ("pc:80:3000", None),
        ("pc:80:3000+book", None),
        ("ab:2+book", None),
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
        "greedy:c10",
        "random:c10",
        "random+book",
        "greedy+book",
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

/// The agent built from a clocked name follows each clock update; a flat
/// name keeps its fixed limits. Checked through `SearchAgent::limits`,
/// with no real time involved.
#[test]
fn clocked_wiring() {
    let board = Board::START;
    let threshold = SearchConfig::standard().endgame_threshold;
    let mut clocked = search("pc:80:c50000", 0, true).unwrap();
    for remaining in [40_000, 20_000] {
        clocked.clock(remaining);
        let (got, want) = (clocked.limits(board), allot(board, remaining, threshold));
        assert_eq!((got.start_ms, got.time_ms), (want.start_ms, want.time_ms));
    }
    let mut flat = search("pc:80:3000", 0, true).unwrap();
    flat.clock(40_000);
    assert_eq!(flat.limits(board).time_ms, 3000);
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

/// `othello-tools` with a fixed load and, if given, a fixed charge per call.
fn command(load: &str, elapsed_ms: Option<&str>, args: &[String]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_othello-tools"));
    command.env("OTHELLO_TOOLS_LOAD", load).args(args);
    match elapsed_ms {
        Some(elapsed) => command.env("OTHELLO_TOOLS_ELAPSED_MS", elapsed),
        None => command.env_remove("OTHELLO_TOOLS_ELAPSED_MS"),
    };
    command
}

fn run(load: &str, elapsed_ms: Option<&str>, args: &[String]) -> Output {
    command(load, elapsed_ms, args).output().unwrap()
}

fn match_args(a: &str, b: &str, pairs: usize, out: &Path, max_load: &str) -> Vec<String> {
    let (pairs, out) = (pairs.to_string(), out.to_str().unwrap());
    [
        "match", "--a", a, "--b", b, "--pairs", &pairs, "--seed", "7",
    ]
    .into_iter()
    .chain(["--jobs", "1", "--max-load", max_load, "--out", out])
    .map(String::from)
    .collect()
}

/// Journal records by id, after any discards.
fn records(path: &Path) -> BTreeMap<String, String> {
    read(path)
        .unwrap()
        .1
        .into_iter()
        .map(|r| (r.id, r.line))
        .collect()
}

fn header(path: &Path) -> String {
    read(path).unwrap().0
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

/// `ab:2:c10` charged 20 ms a move flags on its first move, whatever it
/// would have played.
const FLAGGER: &str = "ab:2:c10";

#[test]
fn journal_fields() {
    let path = temp("fields");
    let done = run(
        "0",
        Some("3"),
        &match_args("pc:80:c200", "greedy", 2, &path, "1000"),
    );
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let header = header(&path);
    assert_eq!(value(&header, "control"), "clock");
    assert_eq!(value(&header, "elapsed"), "3");
    let lines = records(&path);
    assert_eq!(lines.len(), 4);
    for line in lines.values() {
        assert_eq!(value(line, "flag"), "none");
        for side in ["black_ms", "white_ms"] {
            let charged: u64 = value(line, side).parse().unwrap();
            assert!(charged > 0 && charged.is_multiple_of(3), "{side} {charged}");
        }
    }
    fs::remove_file(path).unwrap();
}

/// Every game flags; the journal keeps the stopped board's score in
/// centidiscs, and the report scores the flagger 0 and states times.
#[test]
fn flags_scored() {
    let path = temp("flags");
    let done = run(
        "0",
        Some("20"),
        &match_args(FLAGGER, "greedy", 2, &path, "1000"),
    );
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let openings = positions::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bench/openings_validation.txt"
    ))
    .unwrap();
    for (id, line) in records(&path) {
        let (pair, game) = id.split_once(':').unwrap();
        let opening = &openings[pair.parse::<usize>().unwrap()].squares;
        let (flag, plies) = if game == "0" {
            ("black", 0)
        } else {
            ("white", 1)
        };
        assert_eq!(value(&line, "flag"), flag, "{id}");
        let score: i32 = value(&line, "score").parse().unwrap();
        assert_eq!(score, greedy_lead(opening, plies), "{id}");
    }
    let text = String::from_utf8_lossy(&report(&path).stdout).to_string();
    assert!(text.contains("mean 0.0000"), "{text}");
    assert!(text.contains("time a 20 b 10 flags a 4 b 0"), "{text}");
    fs::remove_file(path).unwrap();
}

/// A flagged pair above the load limit is void like any other.
#[test]
fn flagged_void() {
    let path = temp("void");
    let done = run(
        "9",
        Some("20"),
        &match_args(FLAGGER, "greedy", 2, &path, "7"),
    );
    assert!(!done.status.success());
    let stderr = String::from_utf8_lossy(&done.stderr);
    assert!(stderr.contains("void after ten passes"), "{stderr}");
    assert!(stderr.contains("void pair 0 load 9.000"), "{stderr}");
    assert!(records(&path).is_empty());
    fs::remove_file(path).unwrap();
}

/// One load value: the measured load and the configured limit are both
/// rounded to the journal's three decimals before they are compared, live,
/// on resume and in the report, for clocked and flat timed play.
#[test]
fn load_rounded() {
    for (a, elapsed) in [(FLAGGER, Some("20")), ("ab:2:1", None)] {
        let path = temp("load");
        let args = match_args(a, "greedy", 2, &path, "5.9996");
        let live = run("6.0004", elapsed, &args);
        assert!(
            live.status.success(),
            "{a}: {}",
            String::from_utf8_lossy(&live.stderr)
        );
        assert_eq!(value(&header(&path), "max_load"), "6.000");
        assert!(records(&path).values().all(|l| value(l, "load") == "6.000"));
        let before = fs::read(&path).unwrap();
        assert!(run("6.0004", elapsed, &args).status.success());
        assert_eq!(
            fs::read(&path).unwrap(),
            before,
            "{a}: resume keeps the pairs"
        );
        let summary = report(&path);
        assert!(summary.status.success(), "{a}");
        assert!(String::from_utf8_lossy(&summary.stdout).contains("pairs 2"));
        fs::remove_file(&path).unwrap();
        assert!(
            !run("6.0006", elapsed, &args).status.success(),
            "{a}: 6.001 > 6"
        );
        fs::remove_file(&path).unwrap();
    }
}

/// An interrupted journal with a complete pair and a half pair: the
/// complete pair is kept, the half pair is replayed with its mate.
#[test]
fn half_pair_recovered() {
    let path = temp("half");
    let args = match_args(FLAGGER, "greedy", 3, &path, "1000");
    assert!(run("1", Some("20"), &args).status.success());
    let (head, all) = read(&path).unwrap();
    let kept: Vec<String> = all
        .iter()
        .filter(|r| ["0:0", "0:1", "1:0"].contains(&r.id.as_str()))
        .map(|r| format!("{}\t{}\n", r.id, r.line))
        .collect();
    fs::write(&path, format!("{head}\n{}", kept.concat())).unwrap();
    assert!(run("2", Some("20"), &args).status.success());
    let after = records(&path);
    assert_eq!(after.len(), 6);
    for (id, load) in [("0:0", "1.000"), ("0:1", "1.000"), ("1:0", "2.000")]
        .into_iter()
        .chain([("1:1", "2.000"), ("2:0", "2.000"), ("2:1", "2.000")])
    {
        assert_eq!(value(&after[id], "load"), load, "{id}");
    }
    fs::remove_file(path).unwrap();
}

/// The clocks and the injected charge are part of the journal's identity.
#[test]
fn clock_identity() {
    let path = temp("identity");
    let args = match_args(FLAGGER, "greedy", 2, &path, "1000");
    assert!(run("0", Some("20"), &args).status.success());
    let changed = match_args("ab:2:c11", "greedy", 2, &path, "1000");
    for (elapsed, args) in [(Some("20"), &changed), (Some("21"), &args), (None, &args)] {
        let refused = run("0", elapsed, args);
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("mismatch"));
    }
    fs::remove_file(path).unwrap();
}

/// The clock bounds through the binary. `--pairs 0` plays nothing, so the
/// accepted maximum never starts a days-long search.
#[test]
fn clock_cli_boundary() {
    let path = temp("boundary");
    let top = format!("ab:2:c{MAX_CLOCK_MS}");
    let accepted = run(
        "0",
        Some("1"),
        &match_args(&top, "greedy", 0, &path, "1000"),
    );
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert_eq!(value(&header(&path), "control"), "clock");
    fs::remove_file(&path).unwrap();
    let over = format!("ab:2:c{}", MAX_CLOCK_MS + 1);
    let refused = run(
        "0",
        Some("1"),
        &match_args(&over, "greedy", 0, &path, "1000"),
    );
    assert!(!refused.status.success());
    let mut nodes = match_args("ab:2:c100", "greedy", 0, &path, "1000");
    nodes.extend(["--nodes".into(), "2000".into()]);
    assert!(
        !run("0", Some("1"), &nodes).status.success(),
        "clock with --nodes"
    );
}

/// Kills and reaps `child` before failing, so no run outlives its test.
fn fail(child: &mut Child, message: &str) -> ! {
    let _ = child.kill();
    let _ = child.wait();
    panic!("{message}");
}

/// A clocked match against a slow, depth-limited opponent, SIGKILLed and
/// resumed, ends with the same records and report as an uninterrupted run.
#[test]
fn clock_kill_resume() {
    let (reference, resumed) = (temp("reference"), temp("resumed"));
    let args = |out: &Path| {
        let mut args = match_args("ab:7", FLAGGER, 20, out, "1000");
        args.extend(["--jobs".into(), "2".into()]);
        args
    };
    assert!(run("0", Some("20"), &args(&reference)).status.success());
    let mut child = command("0", Some("20"), &args(&resumed))
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
        if child.try_wait().unwrap().is_some() {
            panic!("run finished before it could be interrupted");
        }
        if start.elapsed() > Duration::from_secs(60) {
            fail(&mut child, "no progress within 60 s");
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(child.wait().unwrap().signal(), Some(9));
    assert!(fs::read_to_string(&resumed).unwrap().lines().count() < 41);
    assert!(run("0", Some("20"), &args(&resumed)).status.success());
    assert_eq!(records(&reference), records(&resumed));
    assert_eq!(report(&reference).stdout, report(&resumed).stdout);
    fs::remove_file(reference).unwrap();
    fs::remove_file(resumed).unwrap();
}

/// Optional smoke test in real time: an engine with a 5 s game clock
/// allocates from it. Real time can flag on a loaded machine, so it is
/// ignored by default.
#[test]
#[ignore]
fn engine_clock_smoke() {
    let path = temp("engine");
    let done = run(
        "0",
        None,
        &match_args("pc:80:c5000", "greedy", 1, &path, "1000"),
    );
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    for line in records(&path).values() {
        assert_eq!(value(line, "flag"), "none");
    }
    fs::remove_file(path).unwrap();
}

/// Rewrites every record payload of a journal with `edit(id, payload)`.
fn rewrite(path: &Path, edit: impl Fn(&str, &str) -> String) {
    let (head, all) = read(path).unwrap();
    let body: String = all
        .iter()
        .map(|r| format!("{}\t{}\n", r.id, edit(&r.id, &r.line)))
        .collect();
    fs::write(path, format!("{head}\n{body}")).unwrap();
}

/// Clock data is all or nothing per record, and must be valid: a damaged
/// record is refused, naming it, rather than counted as zero.
#[test]
fn report_malformed() {
    let damages: [(&str, &str, &str); 3] = [
        ("black_ms 20", "black_ms broken", "invalid"),
        ("flag black", "flag unknown", "invalid"),
        (" white_ms 0", "", "partial"),
    ];
    for (from, to, word) in damages {
        let path = temp("malformed");
        let done = run(
            "0",
            Some("20"),
            &match_args(FLAGGER, "greedy", 2, &path, "1000"),
        );
        assert!(done.status.success());
        assert!(records(&path)["0:0"].contains(from), "{from}");
        rewrite(&path, |id, line| {
            if id == "0:0" {
                line.replacen(from, to, 1)
            } else {
                line.to_string()
            }
        });
        let refused = report(&path);
        assert!(!refused.status.success(), "{to} accepted");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(
            stderr.contains("record 0:0") && stderr.contains(word),
            "{stderr}"
        );
        fs::remove_file(path).unwrap();
    }
}

/// A timed journal from before clock data still reports its score, with
/// its times unavailable; a clock-control journal without them is refused.
#[test]
fn report_older_format() {
    let strip = |_: &str, line: &str| line.split(" black_ms ").next().unwrap().to_string();
    let path = temp("older");
    assert!(
        run("0", None, &match_args("ab:2:1", "greedy", 2, &path, "1000"))
            .status
            .success()
    );
    rewrite(&path, strip);
    let older = report(&path);
    assert!(
        older.status.success(),
        "{}",
        String::from_utf8_lossy(&older.stderr)
    );
    let text = String::from_utf8_lossy(&older.stdout);
    assert!(
        text.contains("pairs 2") && text.contains("time unavailable"),
        "{text}"
    );
    assert!(!text.contains("time a "), "{text}");
    fs::remove_file(&path).unwrap();
    assert!(
        run(
            "0",
            Some("20"),
            &match_args(FLAGGER, "greedy", 2, &path, "1000")
        )
        .status
        .success()
    );
    rewrite(&path, strip);
    let refused = report(&path);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("missing clock data"));
    fs::remove_file(path).unwrap();
}

/// The injected charge is bounded like a clock, so totals fit.
#[test]
fn elapsed_bounded() {
    let path = temp("elapsed");
    let (top, over) = (MAX_CLOCK_MS.to_string(), (MAX_CLOCK_MS + 1).to_string());
    let args = match_args(FLAGGER, "greedy", 0, &path, "1000");
    assert!(run("0", Some(&top), &args).status.success());
    fs::remove_file(&path).unwrap();
    let refused = run("0", Some(&over), &args);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("OTHELLO_TOOLS_ELAPSED_MS"));
}
