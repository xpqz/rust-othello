#![forbid(unsafe_code)]

use std::path::PathBuf;

use othello_trainer::check::{self, Summary};
use othello_trainer::label;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("check") => check_command(&args[1..]),
        Some("label") => label_command(&args[1..]),
        Some("fit" | "inspect") => {
            println!("not implemented");
            Ok(())
        }
        _ => Err("usage: othello-trainer check|label|fit|inspect".into()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

fn check_command(args: &[String]) -> Result<(), String> {
    if args.len() > 2 {
        return Err("usage: check [dir] [manifest]".into());
    }
    let path = |arg: Option<&String>, default: &str| {
        arg.map_or_else(|| PathBuf::from(default), |arg| PathBuf::from(arg.as_str()))
    };
    let dir = path(
        args.first(),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../data/wthor"),
    );
    let manifest = path(
        args.get(1),
        concat!(env!("CARGO_MANIFEST_DIR"), "/wthor.sha256"),
    );
    let summary = check::run(&dir, &manifest)?;
    print_report(&summary);
    let (agree, total) = summary.metrics.get("real").copied().unwrap_or_default();
    if 100 * agree < 99 * total {
        return Err("real-score agreement below 99%".into());
    }
    Ok(())
}

fn label_command(args: &[String]) -> Result<(), String> {
    let mut dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/wthor"));
    let mut journal = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../data/labels.journal"
    ));
    let mut jobs = 1;
    let mut games = None;
    let mut positional = 0;
    let mut rest = args;
    while let Some((flag, tail)) = rest.split_first() {
        match flag.as_str() {
            "--jobs" => {
                jobs = number(tail.first(), "--jobs")?;
                rest = &tail[1..];
            }
            "--games" => {
                games = Some(number(tail.first(), "--games")?);
                rest = &tail[1..];
            }
            _ if positional == 0 => {
                dir = PathBuf::from(flag.as_str());
                positional = 1;
                rest = tail;
            }
            _ if positional == 1 => {
                journal = PathBuf::from(flag.as_str());
                positional = 2;
                rest = tail;
            }
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    if jobs == 0 {
        return Err("--jobs must be at least 1".into());
    }
    label::run(&dir, &journal, jobs, games)
}

fn number(value: Option<&String>, flag: &str) -> Result<usize, String> {
    value
        .ok_or_else(|| format!("{flag} needs a value"))?
        .parse()
        .map_err(|_| format!("invalid {flag}"))
}

fn print_report(summary: &Summary) {
    for (label, (agree, total)) in &summary.metrics {
        let tail = if *total == 0 {
            String::new()
        } else {
            format!("/{total} {:.2}%", 100.0 * *agree as f64 / *total as f64)
        };
        println!("{label} {agree}{tail}");
    }
    for (p3, files) in &summary.p3_values {
        println!("p3 {p3} files {files}");
    }
}
