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
    assert_eq!(rules::source_error("let x = 1;"), None);
}

#[test]
fn source_scope() {
    rules::check_scope(Path::new(env!("CARGO_MANIFEST_DIR")));
}

#[test]
fn budget_guard() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    rules::check_scope(root);
    let total = rules::count_dir(&root.join("src"));
    println!("engine production lines: {total}");
    assert!(total <= 999, "engine production lines {total} exceed 999");
}
