use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use othello_tools::journal::Journal;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn temp(label: &str) -> PathBuf {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("othello_{}_{}_{}", std::process::id(), id, label))
}

fn tools(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_othello-tools"))
        .args(args)
        .output()
        .unwrap()
}

fn report(paths: &[&Path]) -> Output {
    let mut args = vec!["report".to_string()];
    args.extend(paths.iter().map(|p| p.display().to_string()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    tools(&refs)
}

fn command(out: &Path, jobs: usize, range: Option<&str>, seed: &str) -> Vec<String> {
    let mut args = vec![
        "match".into(),
        "--a".into(),
        "ab:4".into(),
        "--b".into(),
        "greedy".into(),
        "--pairs".into(),
        "6".into(),
        "--nodes".into(),
        "3000".into(),
        "--seed".into(),
        seed.into(),
        "--out".into(),
        out.display().to_string(),
        "--jobs".into(),
        jobs.to_string(),
    ];
    if let Some(range) = range {
        args.push("--pair-range".into());
        args.push(range.into());
    }
    args
}

fn run(args: &[String]) -> Output {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    tools(&refs)
}

fn payloads(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .skip(1)
        .filter_map(|line| {
            line.split_once('\t')
                .map(|(_, payload)| payload.to_string())
        })
        .collect()
}

fn game_logs(path: &Path) -> Vec<String> {
    let mut lines: Vec<String> = payloads(path)
        .into_iter()
        .map(|payload| payload.split(" points ").next().unwrap().to_string())
        .collect();
    lines.sort();
    lines
}

fn replace_load(path: &Path, load: &str) {
    let text = fs::read_to_string(path).unwrap();
    let mut out = String::new();
    for line in text.lines() {
        if let Some((id, payload)) = line.split_once('\t') {
            let mut tokens: Vec<String> = payload.split_whitespace().map(str::to_string).collect();
            if let Some(index) = tokens.iter().position(|token| token == "load") {
                tokens[index + 1] = load.into();
            }
            out.push_str(id);
            out.push('\t');
            out.push_str(&tokens.join(" "));
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    fs::write(path, out).unwrap();
}

#[test]
fn journal_roundtrip_and_torn_line() {
    let path = temp("journal");
    let header = "header seed=1";
    let (mut journal, done) = Journal::open(&path, header).unwrap();
    assert!(done.is_empty());
    journal.append("0:0", "first payload").unwrap();
    journal.append("0:1", "second payload").unwrap();
    drop(journal);

    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, &text[..text.len() - 3]).unwrap();
    let (mut journal, done) = Journal::open(&path, header).unwrap();
    assert_eq!(done, BTreeSet::from(["0:0".to_string()]));
    journal.append("0:1", "second payload").unwrap();
    drop(journal);

    let (_, done) = Journal::open(&path, header).unwrap();
    assert_eq!(done.len(), 2);
    assert!(Journal::open(&path, "other header").is_err());
    fs::remove_file(path).unwrap();
}

#[test]
fn kill_and_resume_matches_uninterrupted() {
    let reference = temp("reference");
    let resumed = temp("resumed");
    let base = command(&reference, 2, None, "7");
    let done = run(&base);
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );

    let args = command(&resumed, 2, None, "7");
    let mut child = Command::new(env!("CARGO_BIN_EXE_othello-tools"))
        .args(&args)
        .stdin(Stdio::null())
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
    child.wait().unwrap();

    let done = run(&args);
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let a = report(&[&reference]);
    let b = report(&[&resumed]);
    assert!(a.status.success() && b.status.success());
    assert_eq!(a.stdout, b.stdout);
    fs::remove_file(reference).unwrap();
    fs::remove_file(resumed).unwrap();
}

#[test]
fn header_mismatch_is_rejected() {
    let path = temp("mismatch");
    let first = run(&command(&path, 1, None, "7"));
    assert!(first.status.success());
    let second = run(&command(&path, 1, None, "8"));
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("header"));
    fs::remove_file(path).unwrap();
}

#[test]
fn jobs_are_deterministic() {
    let one = temp("one");
    let three = temp("three");
    let a = run(&command(&one, 1, None, "7"));
    let b = run(&command(&three, 3, None, "7"));
    assert!(a.status.success() && b.status.success());
    assert_eq!(game_logs(&one), game_logs(&three));
    assert_eq!(report(&[&one]).stdout, report(&[&three]).stdout);
    fs::remove_file(one).unwrap();
    fs::remove_file(three).unwrap();
}

#[test]
fn chunks_combine_and_report_rejects_bad_input() {
    let full = temp("full");
    let first = temp("first");
    let second = temp("second");
    let other = temp("other");
    assert!(run(&command(&full, 1, None, "7")).status.success());
    assert!(run(&command(&first, 1, Some("0..3"), "7")).status.success());
    assert!(
        run(&command(&second, 1, Some("3..6"), "7"))
            .status
            .success()
    );
    assert!(run(&command(&other, 1, Some("0..3"), "8")).status.success());

    let combined = report(&[&first, &second]);
    assert!(
        combined.status.success(),
        "{}",
        String::from_utf8_lossy(&combined.stderr)
    );
    assert_eq!(report(&[&full]).stdout, combined.stdout);

    assert!(!report(&[&first, &first]).status.success());
    assert!(!report(&[&first, &other]).status.success());
    let missing = report(&[&first]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("missing"));

    for path in [full, first, second, other] {
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn voiding_applies_only_to_time_control() {
    let node = temp("node");
    let time = temp("time");
    let header = |path: &Path, a: &str| {
        let args = [
            "match",
            "--a",
            a,
            "--b",
            "greedy",
            "--pairs",
            "2",
            "--seed",
            "7",
            "--jobs",
            "1",
            "--out",
            path.to_str().unwrap(),
        ];
        tools(&args)
    };
    assert!(header(&node, "ab:2").status.success());
    replace_load(&node, "99.000");
    assert!(
        report(&[&node]).status.success(),
        "node pairs must not void"
    );

    assert!(header(&time, "ab:2:1").status.success());
    replace_load(&time, "99.000");
    assert!(!report(&[&time]).status.success());
    assert!(String::from_utf8_lossy(&report(&[&time]).stderr).contains("missing"));

    let args = [
        "match",
        "--a",
        "ab:2:1",
        "--b",
        "greedy",
        "--pairs",
        "2",
        "--seed",
        "7",
        "--jobs",
        "1",
        "--out",
        time.to_str().unwrap(),
    ];
    assert!(tools(&args).status.success());
    assert!(report(&[&time]).status.success());
    fs::remove_file(node).unwrap();
    fs::remove_file(time).unwrap();
}

#[test]
fn bad_arguments_are_rejected() {
    for args in [
        ["match", "--a", "greedy", "--b", "greedy", "--jobs", "0"].as_slice(),
        [
            "match",
            "--a",
            "greedy",
            "--b",
            "greedy",
            "--pair-range",
            "x",
        ]
        .as_slice(),
        [
            "match",
            "--a",
            "greedy",
            "--b",
            "greedy",
            "--pair-range",
            "3..3",
        ]
        .as_slice(),
        [
            "match",
            "--a",
            "greedy",
            "--b",
            "greedy",
            "--pair-range",
            "0..9999",
        ]
        .as_slice(),
        ["report"].as_slice(),
    ] {
        assert!(!tools(args).status.success(), "{args:?}");
    }
}
