use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use othello::board::{Board, is_terminal, legal_moves, pass, play};
use othello::rng::Rng;
use othello_trainer::fit;
use othello_trainer::wthor;
use sha2::{Digest, Sha256};

#[test]
fn fit_restart_and_order() {
    let full = make_root("full", 200);
    let (asset_path, report_path) = run_fit(&full);
    let asset = std::fs::read(&asset_path).unwrap();
    let report = std::fs::read_to_string(&report_path).unwrap();

    let bytes = std::fs::read(full.join("data/wthor/games.wtb")).unwrap();
    let (_, games) = wthor::read(&bytes).unwrap();
    let held = games.iter().filter(|game| fit::held_out(game)).count();
    assert!(held > 0 && held < games.len(), "{held}/{}", games.len());

    let partial = make_root("partial", 200);
    let stages = full.join("data/fit");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&stages)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("stage_"))
        })
        .collect();
    files.sort();
    assert!(files.len() >= 4);
    std::fs::create_dir_all(partial.join("data/fit")).unwrap();
    for path in &files[..2] {
        std::fs::copy(
            path,
            partial.join("data/fit").join(path.file_name().unwrap()),
        )
        .unwrap();
    }
    let mut wrong = std::fs::read_to_string(&files[2]).unwrap();
    wrong.insert(0, '!');
    std::fs::write(
        partial.join("data/fit").join(files[2].file_name().unwrap()),
        wrong,
    )
    .unwrap();
    let mut damaged = std::fs::read(&files[3]).unwrap();
    let last = damaged.len() - 1;
    damaged[last] ^= 0xff;
    std::fs::write(
        partial.join("data/fit").join(files[3].file_name().unwrap()),
        damaged,
    )
    .unwrap();
    std::fs::write(partial.join("data/fit/stage_04.tmp"), b"partial").unwrap();
    let (partial_asset, partial_report) = run_fit(&partial);
    assert_eq!(std::fs::read(partial_asset).unwrap(), asset);
    assert_eq!(std::fs::read_to_string(partial_report).unwrap(), report);

    let killed = make_root("killed", 200);
    let mut child = Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
        .arg("fit")
        .arg(&killed)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if stage_ready(&killed.join("data/fit")) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "no stage file before timeout"
        );
        sleep(Duration::from_millis(5));
    }
    let _ = child.kill();
    let _ = child.wait();
    let (killed_asset, killed_report) = run_fit(&killed);
    assert_eq!(std::fs::read(killed_asset).unwrap(), asset);
    assert_eq!(std::fs::read_to_string(killed_report).unwrap(), report);

    let reordered = make_root("reordered", 200);
    let journal = reordered.join("data/labels.journal");
    let (header, mut records) = othello_tools::journal::read(&journal).unwrap();
    records.reverse();
    let mut text = format!("{header}\n");
    for record in records {
        text.push_str(&format!("{}\t{}\n", record.id, record.line));
    }
    std::fs::write(&journal, text).unwrap();
    let (reordered_asset, _) = run_fit(&reordered);
    assert_eq!(std::fs::read(reordered_asset).unwrap(), asset);
}

fn run_fit(root: &Path) -> (PathBuf, PathBuf) {
    let output = Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
        .arg("fit")
        .arg(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        root.join("assets/eval.bin"),
        root.join("docs/tuning/pattern_fit.log"),
    )
}

fn stage_ready(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("stage_"))
        })
}

fn make_root(name: &str, count: usize) -> PathBuf {
    let root = temp_dir(name);
    let mut seen = BTreeSet::new();
    let mut moves = Vec::new();
    let mut seed = 0;
    while moves.len() < count {
        let game = playout(seed);
        seed += 1;
        if seen.insert(game.clone()) {
            moves.push(game);
        }
    }
    let archive = archive(&moves);
    std::fs::create_dir_all(root.join("data/wthor")).unwrap();
    std::fs::create_dir_all(root.join("data/fit")).unwrap();
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::create_dir_all(root.join("docs/tuning")).unwrap();
    std::fs::create_dir_all(root.join("trainer")).unwrap();
    std::fs::write(root.join("data/wthor/games.wtb"), &archive).unwrap();
    let (_, games) = wthor::read(&archive).unwrap();
    let mut journal = "header test\n".to_string();
    for (index, game) in games.iter().enumerate() {
        let positions = wthor::replay(game).unwrap().positions.len();
        let values: Vec<String> = (0..positions)
            .map(|ply| ((ply % 5) + (index % 3)).to_string())
            .collect();
        journal.push_str(&format!("games.wtb:{index}\t{}\n", values.join(" ")));
    }
    std::fs::write(root.join("data/labels.journal"), journal).unwrap();
    let hash: String = Sha256::digest(&archive)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    std::fs::write(
        root.join("trainer/wthor.sha256"),
        format!("{hash}  games.wtb\n"),
    )
    .unwrap();
    root
}

fn playout(seed: u64) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    let mut board = Board::START;
    let mut moves = Vec::new();
    loop {
        if is_terminal(board) {
            break;
        }
        let legal = legal_moves(board.me, board.opp);
        if legal == 0 {
            board = pass(board);
            continue;
        }
        let pick = rng.next_u64() % legal.count_ones() as u64;
        let mut rest = legal;
        for _ in 0..pick {
            rest &= rest - 1;
        }
        let square = rest.trailing_zeros() as u8;
        moves.push(square % 8 + 1 + 10 * (square / 8 + 1));
        board = play(board, square);
    }
    moves
}

fn archive(games: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0; 16];
    bytes[4..8].copy_from_slice(&(games.len() as u32).to_le_bytes());
    bytes[10..12].copy_from_slice(&2025u16.to_le_bytes());
    bytes[12] = 8;
    bytes[14] = 24;
    for moves in games {
        let mut record = [0; 68];
        record[6] = 32;
        record[7] = 32;
        record[8..8 + moves.len()].copy_from_slice(moves);
        bytes.extend_from_slice(&record);
    }
    bytes
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("othello-fit-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
