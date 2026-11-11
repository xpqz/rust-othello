use std::fs;
use std::path::{Component, Path, PathBuf};

fn files(dir: &Path) -> Vec<PathBuf> {
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

/// Counts formatted non-blank lines that are not comment-only.
pub fn count_text(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with("//")
        })
        .count()
}

pub fn count_dir(dir: &Path) -> usize {
    files(dir)
        .iter()
        .map(|path| count_text(&fs::read_to_string(path).unwrap()))
        .sum()
}

/// Substring checks may reject documentation that mentions these spellings.
pub fn source_error(text: &str) -> Option<&'static str> {
    if text.contains("/*") {
        return Some("block comment");
    }
    for line in text.lines() {
        if line.contains("#[test]") || (line.contains("cfg") && line.contains("test")) {
            return Some("test code");
        }
        if line.contains("include!(") {
            return Some("include macro");
        }
        if line.contains("include") && line.contains('!') && !asset_include(line) {
            return Some("include macro");
        }
        if line.contains("#[path") {
            return Some("#[path]");
        }
    }
    None
}

fn asset_include(line: &str) -> bool {
    let Some((_, rest)) = line.split_once("include_bytes!(") else {
        return false;
    };
    let Some(end) = rest.find(')') else {
        return false;
    };
    let literal = rest[..end].trim();
    let Some(literal) = literal
        .strip_prefix('"')
        .and_then(|literal| literal.strip_suffix('"'))
    else {
        return false;
    };
    let path = Path::new("src").join(literal);
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized.starts_with("assets")
}

pub fn check_scope(root: &Path) {
    assert!(!root.join("build.rs").exists(), "build.rs is not allowed");
    assert!(!root.join("rustfmt.toml").exists());
    assert!(!root.join(".rustfmt.toml").exists());
    for path in files(&root.join("src")) {
        let text = fs::read_to_string(&path).unwrap();
        if let Some(reason) = source_error(&text) {
            panic!("{}: {reason}", path.display());
        }
    }
}
