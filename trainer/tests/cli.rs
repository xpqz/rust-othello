use std::process::Command;

fn trainer(command: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_othello-trainer"))
        .arg(command)
        .output()
        .unwrap()
}

#[test]
fn commands_work() {
    for command in ["check", "label", "fit", "inspect"] {
        let output = trainer(command);
        assert!(output.status.success(), "{command}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "not implemented\n");
    }
}

#[test]
fn unknown_command() {
    let output = trainer("unknown");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("usage:"));
}
