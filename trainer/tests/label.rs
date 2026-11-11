use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use othello::board::{Board, empty_count};
use othello::score::INF;
use othello::search::{self, Ctx, SearchConfig};
use othello_trainer::label;
use othello_trainer::wthor::{self, Game, Replay};

const GAME: [u8; 59] = [
    56, 64, 73, 66, 43, 34, 46, 36, 33, 65, 67, 22, 11, 75, 24, 74, 37, 42, 51, 53, 57, 48, 35, 23,
    86, 62, 76, 87, 71, 77, 88, 72, 32, 28, 14, 52, 61, 31, 82, 84, 25, 47, 26, 27, 38, 21, 78, 58,
    13, 85, 17, 15, 68, 12, 18, 16, 41, 63, 83,
];

fn board(empty: u32) -> Board {
    let discs = 64 - empty;
    Board {
        me: if discs == 64 {
            u64::MAX
        } else {
            (1u64 << discs) - 1
        },
        opp: 0,
    }
}

fn game(real: u8, theoretical: u8) -> Game {
    Game {
        real,
        theoretical,
        moves: Vec::new(),
    }
}

fn replay(positions: Vec<(Board, bool)>, final_board: Board, final_black: bool) -> Replay {
    Replay {
        positions,
        final_board,
        final_black,
    }
}

fn solve_with(position: Board, ctx: &mut Ctx) -> Result<i32, String> {
    search::solve(position, -INF, INF, ctx).ok_or("solve failed".into())
}

#[test]
fn label_bands() {
    let board24 = board(24);
    let board30 = board(30);
    let board23 = board(23);
    let board15 = board(15);
    let board14 = board(14);
    let board10 = board(10);
    let replay = replay(
        vec![
            (board24, true),
            (board30, false),
            (board23, false),
            (board15, true),
            (board14, false),
            (board10, true),
        ],
        board(0),
        true,
    );
    let mut calls = Vec::new();
    let mut solve = |position: Board| {
        calls.push(position);
        Ok(if empty_count(position) == 10 {
            -1300
        } else {
            4200
        })
    };
    let labels = label::labels(24, &game(56, 37), &replay, &mut solve).unwrap();
    assert_eq!(labels[0].label, 10);
    assert_eq!(labels[1].label, -10);
    assert_eq!(labels[2].label, 42);
    assert_eq!(labels[3].label, -42);
    assert_eq!(labels[4].label, 42);
    assert_eq!(labels[5].label, -13);
    assert_eq!(calls, vec![board14, board10]);
}

#[test]
fn colour_flip() {
    let before = board(20);
    let after = board(20);
    let anchor = board(14);
    let replay = replay(
        vec![(before, true), (after, false), (anchor, false)],
        board(0),
        true,
    );
    let mut calls = 0;
    let mut solve = |_position: Board| {
        calls += 1;
        Ok(4200)
    };
    let labels = label::labels(24, &game(56, 0), &replay, &mut solve).unwrap();
    assert_eq!(labels[0].label, -42);
    assert_eq!(labels[1].label, 42);
    assert_eq!(labels[2].label, 42);
    assert_eq!(calls, 1);
}

#[test]
fn final_result() {
    let board20 = board(20);
    let replay = replay(vec![(board20, true), (board20, false)], board(20), true);
    let mut calls = 0;
    let mut solve = |_position: Board| {
        calls += 1;
        Ok(0)
    };
    let labels = label::labels(24, &game(40, 0), &replay, &mut solve).unwrap();
    assert_eq!(labels[0].label, 16);
    assert_eq!(labels[1].label, -16);
    assert_eq!(calls, 0);
}

#[test]
fn disc_units() {
    let replay = replay(vec![(board(10), true)], board(0), true);
    let mut solve = |_position: Board| Ok(4200);
    assert_eq!(
        label::labels(24, &game(0, 0), &replay, &mut solve).unwrap()[0].label,
        42
    );
    let mut solve = |_position: Board| Ok(-1300);
    assert_eq!(
        label::labels(24, &game(0, 0), &replay, &mut solve).unwrap()[0].label,
        -13
    );
}

#[test]
fn pass_chain() {
    let (p3, games) = wthor::read(&archive(1, None)).unwrap();
    let replay = wthor::replay(&games[0]).unwrap();
    let config = SearchConfig::standard();
    let mut ctx = Ctx::new(&config);
    let mut solver = |position| solve_with(position, &mut ctx);
    let labels = label::labels(p3, &games[0], &replay, &mut solver).unwrap();
    let chain: Vec<(bool, i32)> = labels
        .iter()
        .filter(|label| empty_count(label.board) <= 10)
        .map(|label| {
            (
                label.black,
                if label.black {
                    label.label
                } else {
                    -label.label
                },
            )
        })
        .collect();
    assert!(chain.len() > 2);
    for pair in chain.windows(2) {
        let (black, value) = pair[0];
        let next = pair[1].1;
        if black {
            assert!(next >= value, "{value} fell to {next} after black");
        } else {
            assert!(next <= value, "{value} rose to {next} after white");
        }
    }
    assert_eq!(chain.last().unwrap().1, 48);
}

#[test]
fn journal_roundtrip() {
    let dir = temp_dir("roundtrip");
    let bytes = archive(1, None);
    std::fs::write(dir.join("games.wtb"), &bytes).unwrap();
    let journal = dir.join("labels.journal");
    let output = trainer(&[
        "label",
        dir.to_str().unwrap(),
        journal.to_str().unwrap(),
        "--jobs",
        "1",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (_, records) = othello_tools::journal::read(&journal).unwrap();
    assert_eq!(records.len(), 1);
    let (p3, games) = wthor::read(&bytes).unwrap();
    let replay = wthor::replay(&games[0]).unwrap();
    let config = SearchConfig::standard();
    let mut ctx = Ctx::new(&config);
    let mut solver = |position| solve_with(position, &mut ctx);
    let expected: Vec<i32> = label::labels(p3, &games[0], &replay, &mut solver)
        .unwrap()
        .into_iter()
        .map(|label| label.label)
        .collect();
    assert_eq!(label::decode(&records[0].line).unwrap(), expected);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn games_limit() {
    let dir = temp_dir("limit");
    std::fs::write(dir.join("games.wtb"), archive(3, None)).unwrap();
    let journal = dir.join("labels.journal");
    let output = trainer(&[
        "label",
        dir.to_str().unwrap(),
        journal.to_str().unwrap(),
        "--jobs",
        "1",
        "--games",
        "2",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (_, records) = othello_tools::journal::read(&journal).unwrap();
    assert_eq!(records.len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn journal_resume() {
    let dir = temp_dir("journal");
    let valid = 2;
    std::fs::write(dir.join("games.wtb"), archive(valid + 1, Some(valid))).unwrap();
    let dir_arg = dir.to_str().unwrap();
    let full = dir.join("full.journal");
    let full_arg = full.to_str().unwrap();
    let output = trainer(&["label", dir_arg, full_arg, "--jobs", "1"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = sorted_lines(&full);
    assert_eq!(expected.len(), valid);

    let killed = dir.join("killed.journal");
    let killed_arg = killed.to_str().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
        .args(["label", dir_arg, killed_arg, "--jobs", "1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Ok((_, records)) = othello_tools::journal::read(&killed)
            && !records.is_empty()
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "no journal line before timeout"
        );
        sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let status = child.wait().unwrap();
    assert_eq!(status.signal(), Some(9));
    assert!(sorted_lines(&killed).len() < valid);
    let output = trainer(&["label", dir_arg, killed_arg, "--jobs", "2"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(sorted_lines(&killed), expected);

    let mut changed = std::fs::read(dir.join("games.wtb")).unwrap();
    changed[10] ^= 1;
    std::fs::write(dir.join("games.wtb"), changed).unwrap();
    let output = trainer(&["label", dir_arg, full_arg, "--jobs", "1"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("header"));
    std::fs::remove_dir_all(dir).unwrap();
}

fn record() -> [u8; 68] {
    let mut record = [0; 68];
    record[6] = 56;
    record[7] = 56;
    record[8..8 + GAME.len()].copy_from_slice(&GAME);
    record
}

fn invalid_record() -> [u8; 68] {
    let mut record = [0; 68];
    record[8] = 11;
    record
}

fn archive(games: usize, bad: Option<usize>) -> Vec<u8> {
    let mut bytes = vec![0; 16];
    bytes[4..8].copy_from_slice(&(games as u32).to_le_bytes());
    bytes[10..12].copy_from_slice(&2025u16.to_le_bytes());
    bytes[12] = 8;
    bytes[14] = 24;
    for index in 0..games {
        if Some(index) == bad {
            bytes.extend_from_slice(&invalid_record());
        } else {
            bytes.extend_from_slice(&record());
        }
    }
    bytes
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("othello-label-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn trainer(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
        .args(args)
        .output()
        .unwrap()
}

fn sorted_lines(path: &Path) -> Vec<String> {
    let (_, records) = othello_tools::journal::read(path).unwrap();
    let mut lines: Vec<String> = records
        .iter()
        .map(|record| format!("{}\t{}", record.id, record.line))
        .collect();
    lines.sort();
    lines
}
