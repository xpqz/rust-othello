use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files
}

fn code_lines(text: &str) -> usize {
    let mut count = 0;
    let mut block = false;
    for line in text.lines() {
        let mut code = String::new();
        let mut rest = line;
        loop {
            if block {
                if let Some(end) = rest.find("*/") {
                    rest = &rest[end + 2..];
                    block = false;
                } else {
                    break;
                }
            } else if let Some(start) = rest.find("/*") {
                code.push_str(&rest[..start]);
                rest = &rest[start + 2..];
                block = true;
            } else {
                code.push_str(rest);
                break;
            }
        }
        let code = code.trim();
        if !code.is_empty() && !code.starts_with("//") {
            count += 1;
        }
    }
    count
}

#[test]
fn tools_scope() {
    assert!(!Path::new("build.rs").exists(), "build.rs is not allowed");
    assert!(!Path::new("rustfmt.toml").exists());
    assert!(!Path::new(".rustfmt.toml").exists());
    for path in rust_files(Path::new("src")) {
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("#[cfg(test)]"),
            "{} contains test code",
            path.display()
        );
        assert!(
            !text.contains("include!"),
            "{} uses include!",
            path.display()
        );
        assert!(!text.contains("#[path"), "{} uses #[path]", path.display());
    }
}

#[test]
fn tools_budget() {
    let total: usize = rust_files(Path::new("src"))
        .iter()
        .map(|path| code_lines(&fs::read_to_string(path).unwrap()))
        .sum();
    println!("tools production lines: {total}");
    assert!(total <= 600, "tools production lines {total} exceed 600");
}
