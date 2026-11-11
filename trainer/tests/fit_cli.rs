use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

const GAME: [u8; 59] = [
    56, 64, 73, 66, 43, 34, 46, 36, 33, 65, 67, 22, 11, 75, 24, 74, 37, 42, 51, 53, 57, 48, 35, 23,
    86, 62, 76, 87, 71, 77, 88, 72, 32, 28, 14, 52, 61, 31, 82, 84, 25, 47, 26, 27, 38, 21, 78, 58,
    13, 85, 17, 15, 68, 12, 18, 16, 41, 63, 83,
];

#[test]
fn fit_restart_and_order() {
    let dir = temp_dir("fit");
    let games = 200;
    let archive = archive(games);
    std::fs::write(dir.join("games.wtb"), &archive).unwrap();
    let labels = dir.join("labels.journal");
    let mut journal = "header test\n".to_string();
    for index in 0..games {
        let values: Vec<String> = (0..59)
            .map(|ply| ((ply % 5) + (index % 3)).to_string())
            .collect();
        journal.push_str(&format!("games.wtb:{index}\t{}\n", values.join(" ")));
    }
    std::fs::write(&labels, journal).unwrap();

    let manifest = dir.join("wthor.sha256");
    let hash: String = Sha256::digest(&archive)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    std::fs::write(&manifest, format!("{hash}  games.wtb\n")).unwrap();

    let full = dir.join("full");
    let (full_asset, full_report) = run_fit(&dir, &labels, &manifest, &full);
    let asset = std::fs::read(&full_asset).unwrap();
    let report = std::fs::read_to_string(&full_report).unwrap();

    let killed = dir.join("killed");
    std::fs::create_dir_all(killed.join("stages")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
        .args(fit_args(&dir, &labels, &manifest, &killed))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if stage_ready(&killed.join("stages")) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "no stage file before timeout"
        );
        sleep(Duration::from_millis(5));
    }
    let _ = child.kill();
    let status = child.wait().unwrap();
    assert_eq!(status.signal(), Some(9));
    assert!(!killed.join("eval.bin").exists());
    let (killed_asset, killed_report) = run_fit(&dir, &labels, &manifest, &killed);
    assert_eq!(std::fs::read(killed_asset).unwrap(), asset);
    assert_eq!(std::fs::read_to_string(killed_report).unwrap(), report);

    let reversed = dir.join("reversed.journal");
    let (header, mut records) = othello_tools::journal::read(&labels).unwrap();
    records.reverse();
    let mut text = format!("{header}\n");
    for record in records {
        text.push_str(&format!("{}\t{}\n", record.id, record.line));
    }
    std::fs::write(&reversed, text).unwrap();
    let reordered = dir.join("reordered");
    let (reordered_asset, _) = run_fit(&dir, &reversed, &manifest, &reordered);
    assert_eq!(std::fs::read(reordered_asset).unwrap(), asset);

    std::fs::remove_dir_all(dir).unwrap();
}

fn run_fit(dir: &Path, labels: &Path, manifest: &Path, out: &Path) -> (PathBuf, PathBuf) {
    std::fs::create_dir_all(out.join("stages")).unwrap();
    let output = bin_strings(&fit_args(dir, labels, manifest, out));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (out.join("eval.bin"), out.join("pattern_fit.log"))
}

fn fit_args(dir: &Path, labels: &Path, manifest: &Path, out: &Path) -> Vec<String> {
    vec![
        "fit".into(),
        "--dir".into(),
        dir.to_str().unwrap().into(),
        "--labels".into(),
        labels.to_str().unwrap().into(),
        "--manifest".into(),
        manifest.to_str().unwrap().into(),
        "--stages".into(),
        out.join("stages").to_str().unwrap().into(),
        "--asset".into(),
        out.join("eval.bin").to_str().unwrap().into(),
        "--layout".into(),
        out.join("eval.layout").to_str().unwrap().into(),
        "--report".into(),
        out.join("pattern_fit.log").to_str().unwrap().into(),
    ]
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
    let dir = std::env::temp_dir().join(format!("othello-fit-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn bin_strings(args: &[String]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
        .args(args)
        .output()
        .unwrap()
}
