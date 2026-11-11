#![forbid(unsafe_code)]

use std::path::PathBuf;

use othello_trainer::check::{self, Summary};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("check") => check_command(&args[1..]),
        Some("label" | "fit" | "inspect") => {
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
