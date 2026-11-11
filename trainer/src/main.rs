#![forbid(unsafe_code)]

use std::path::PathBuf;

use othello_trainer::check::{self, Summary};
use othello_trainer::fit;
use othello_trainer::label;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("check") => check_command(&args[1..]),
        Some("label") => label_command(&args[1..]),
        Some("fit") => fit_command(&args[1..]),
        Some("inspect") => inspect_command(&args[1..]),
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
    let (mut dir, mut journal) = (None, None);
    let (mut jobs, mut games) = (1, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--jobs" => jobs = number(args.next(), "--jobs")?,
            "--games" => games = Some(number(args.next(), "--games")?),
            _ if dir.is_none() => dir = Some(PathBuf::from(arg.as_str())),
            _ if journal.is_none() => journal = Some(PathBuf::from(arg.as_str())),
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    if jobs == 0 {
        return Err("--jobs must be at least 1".into());
    }
    let dir =
        dir.unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/wthor")));
    let journal = journal.unwrap_or_else(|| {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/labels.journal"
        ))
    });
    label::run(&dir, &journal, jobs, games)
}

fn fit_command(args: &[String]) -> Result<(), String> {
    let mut root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    let mut options = fit::Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--step" => options.step = real(args.next(), "--step")?,
            "--floor" => options.floor = real(args.next(), "--floor")?,
            "--patience" => options.patience = number(args.next(), "--patience")?,
            "--max-passes" => options.max_passes = number(args.next(), "--max-passes")?,
            _ => root = PathBuf::from(arg.as_str()),
        }
    }
    fit::run(&root, &options)
}

fn inspect_command(args: &[String]) -> Result<(), String> {
    let names = [
        "edge",
        "corner3",
        "corner2x5",
        "row2",
        "row3",
        "row4",
        "diag8",
        "diag7",
        "diag6",
        "diag5",
        "diag4",
    ];
    let mut root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    let mut family = 0;
    let mut stage = 0;
    let mut count = 10;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--family" => {
                let name = args.next().ok_or("--family needs a value")?;
                family = names
                    .iter()
                    .position(|family| *family == name.as_str())
                    .ok_or_else(|| format!("unknown family {name}"))?;
            }
            "--stage" => stage = number(args.next(), "--stage")?,
            "--count" => count = number(args.next(), "--count")?,
            _ => root = PathBuf::from(arg.as_str()),
        }
    }
    print!("{}", fit::inspect_path(&root, family, stage, count)?);
    Ok(())
}

fn real(value: Option<&String>, flag: &str) -> Result<f32, String> {
    value
        .ok_or_else(|| format!("{flag} needs a value"))?
        .parse()
        .map_err(|_| format!("invalid {flag}"))
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
