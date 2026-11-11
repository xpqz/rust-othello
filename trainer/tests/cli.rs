use std::process::Command;

use sha2::{Digest, Sha256};

fn bin(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn inspect_not_implemented() {
    let output = bin(&["inspect"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "not implemented\n");
}

#[test]
fn check_synthetic() {
    let dir = std::env::temp_dir().join(format!("othello-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut bytes = vec![0u8; 16];
    bytes[12] = 8;
    bytes[14] = 24;
    std::fs::write(dir.join("empty.wtb"), &bytes).unwrap();
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let manifest = dir.join("wthor.sha256");
    std::fs::write(&manifest, format!("{hash}  empty.wtb\n")).unwrap();
    let output = bin(&["check", dir.to_str().unwrap(), manifest.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("games 0"), "{text}");
    assert!(text.contains("p3 24 files 1"), "{text}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unknown_command() {
    let output = bin(&["unknown"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("usage:"));
}
