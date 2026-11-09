#[path = "../../tests/budget/rules.rs"]
mod rules;

use std::path::Path;

#[test]
fn tools_scope() {
    rules::check_scope(Path::new(env!("CARGO_MANIFEST_DIR")));
}

#[test]
fn tools_budget() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let total = rules::count_dir(&root.join("src"));
    println!("tools production lines: {total}");
    assert!(total <= 1200, "tools production lines {total} exceed 1200");
}
