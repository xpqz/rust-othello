#[path = "../../tests/budget/rules.rs"]
mod rules;

#[test]
fn trainer_scope() {
    rules::check_scope(std::path::Path::new(env!("CARGO_MANIFEST_DIR")));
}

#[test]
fn trainer_budget() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let total = rules::count_dir(&root.join("src"));
    println!("trainer production lines: {total}");
    assert!(total <= 450, "trainer production lines {total} exceed 450");
}
