#[path = "budget/rules.rs"]
mod rules;

use std::path::Path;

#[test]
fn fixture_lines() {
    let fixture = "// comment\n/// doc\n//! inner\n\nlet x = 1; // trailing\n  // indented\n";
    assert_eq!(rules::count_text(fixture), 1);
    assert_eq!(rules::source_error("/* block */"), Some("block comment"));
    assert_eq!(
        rules::source_error("let s = \"include!(\";"),
        Some("include macro")
    );
    assert_eq!(rules::source_error("#[cfg(test)]"), Some("test code"));
    assert_eq!(rules::source_error("#[test]"), Some("test code"));
    assert_eq!(
        rules::source_error("#[cfg_attr(test, allow(dead_code))]"),
        Some("test code")
    );
    assert_eq!(
        rules::source_error("include_bytes!(\"data\")"),
        Some("include macro")
    );
    assert_eq!(
        rules::source_error(r#"include_bytes!("../assets/eval.bin")"#),
        None
    );
    assert_eq!(
        rules::source_error(r#"include_bytes!("../assets/eval.layout")"#),
        None
    );
    assert_eq!(
        rules::source_error(r#"include_bytes!("../data/eval.bin")"#),
        Some("include macro")
    );
    assert_eq!(
        rules::source_error(r#"include!("../assets/eval.bin")"#),
        Some("include macro")
    );
    assert_eq!(
        rules::source_error(r#"include_bytes!("/tmp/eval.bin")"#),
        Some("include macro")
    );
    assert_eq!(
        rules::source_error(r#"include_bytes!("../assets/../src/secret")"#),
        Some("include macro")
    );
    assert_eq!(rules::source_error("let x = 1;"), None);
}

#[test]
fn source_scope() {
    rules::check_scope(Path::new(env!("CARGO_MANIFEST_DIR")));
}

#[test]
fn budget_guard() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let total = rules::count_dir(&root.join("src"));
    println!("engine production lines: {total}");
    assert!(total <= 1499, "engine production lines {total} exceed 1499");
}

#[test]
fn engine_budget() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let total = rules::count_dir(&root.join("src"));
    println!("engine production lines: {total}");
    assert!(
        total <= 1150,
        "engine production lines {total} exceed 1150 after the switch"
    );
}

#[test]
fn asset_budget() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut total = 0u64;
    let mut pending = vec![root.join("assets")];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if let Ok(metadata) = path.metadata() {
                total += metadata.len();
            }
        }
    }
    println!("assets bytes: {total}");
    assert!(
        total <= 4 * 1024 * 1024,
        "assets total {total} exceeds 4 MiB"
    );
}

#[test]
fn no_engine_deps() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = std::fs::read_to_string(path).unwrap();
    for line in text.lines() {
        let line = line.trim();
        let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        else {
            continue;
        };
        let name = name.trim();
        assert!(
            !name.split('.').any(|part| matches!(
                part,
                "dependencies" | "dev-dependencies" | "build-dependencies"
            )),
            "engine dependency table: {name}"
        );
    }
}
