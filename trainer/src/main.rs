#![forbid(unsafe_code)]

use std::path::PathBuf;

use othello_trainer::check::{self, Summary};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(message) = run(&args) {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.split_first() {
        Some((command, rest)) if command == "check" => check_command(rest),
        Some((command, _)) if matches!(command.as_str(), "label" | "fit" | "inspect") => {
            println!("not implemented");
            Ok(())
        }
        _ => Err("usage: othello-trainer check|label|fit|inspect".into()),
    }
}

fn check_command(args: &[String]) -> Result<(), String> {
    let mut dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/wthor"));
    let mut manifest = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/wthor.sha256"));
    let mut rest = args;
    while let Some((flag, tail)) = rest.split_first() {
        let value = tail.first().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--dir" => dir = PathBuf::from(value.as_str()),
            "--manifest" => manifest = PathBuf::from(value.as_str()),
            _ => return Err(format!("unknown option {flag}")),
        }
        rest = &tail[1..];
    }
    let summary = check::run(&dir, &manifest)?;
    print_report(&summary);
    if 100 * summary.real_agree < 99 * summary.replayed {
        return Err("real-score agreement below 99%".into());
    }
    Ok(())
}

fn print_report(summary: &Summary) {
    println!("games {}", summary.games_read);
    println!("replayed {}", summary.replayed);
    println!("rejected illegal {}", summary.rejected_illegal);
    println!("rejected incomplete {}", summary.rejected_incomplete);
    for (p3, files) in &summary.p3_values {
        println!("p3 {p3} files {files}");
    }
    line("real", summary.real_agree, summary.replayed);
    line(
        "full board",
        summary.full_board.agree,
        summary.full_board.total,
    );
    line(
        "empty board",
        summary.empty_board.agree,
        summary.empty_board.total,
    );
    line(
        "empty black win",
        summary.black_win.agree,
        summary.black_win.total,
    );
    line(
        "empty black loss",
        summary.black_loss.agree,
        summary.black_loss.total,
    );
    line("theoretical", summary.theoretical_equal, summary.replayed);
}

fn line(label: &str, agree: usize, total: usize) {
    let pct = if total == 0 {
        0.0
    } else {
        100.0 * agree as f64 / total as f64
    };
    println!("{label} {agree}/{total} {pct:.2}%");
}
