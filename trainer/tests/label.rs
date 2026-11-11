use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use othello::board::{Board, empty_count};
use othello_trainer::label;
use othello_trainer::wthor::{Game, Replay};

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

#[test]
fn label_bands() {
    let board30 = board(30);
    let board20 = board(20);
    let board14 = board(14);
    let board10 = board(10);
    let replay = replay(
        vec![
            (board30, true),
            (board20, false),
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
    assert_eq!(labels[1].label, 42);
    assert_eq!(labels[2].label, 42);
    assert_eq!(labels[3].label, -13);
    assert_eq!(calls, vec![board14, board10]);
}

#[test]
fn pass_sign() {
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
fn journal_resume() {
    let dir = temp_dir("journal");
    let games = 4;
    std::fs::write(dir.join("games.wtb"), archive(games)).unwrap();
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
    assert_eq!(expected.len(), games);

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
    assert!(sorted_lines(&killed).len() < games);
    let output = trainer(&["label", dir_arg, killed_arg, "--jobs", "1"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(sorted_lines(&killed), expected);
    std::fs::remove_dir_all(dir).unwrap();
}

fn record() -> [u8; 68] {
    let mut record = [0; 68];
    record[6] = 56;
    record[7] = 56;
    record[8..8 + GAME.len()].copy_from_slice(&GAME);
    record
}

fn archive(games: usize) -> Vec<u8> {
    let mut bytes = vec![0; 16];
    bytes[4..8].copy_from_slice(&(games as u32).to_le_bytes());
    bytes[10..12].copy_from_slice(&2025u16.to_le_bytes());
    bytes[12] = 8;
    bytes[14] = 24;
    for _ in 0..games {
        bytes.extend_from_slice(&record());
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
