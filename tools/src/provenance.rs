use std::fs;
use std::process::Command;

pub fn file_hash(path: &str) -> Result<u64, String> {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in fs::read(path).map_err(|error| format!("{path}: {error}"))? {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Ok(hash)
}

pub fn git_commit() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}
