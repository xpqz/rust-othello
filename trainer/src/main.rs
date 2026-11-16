#![forbid(unsafe_code)]

use std::path::PathBuf;

use othello_trainer::book::{self, Params};
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
        Some("book") => book_command(&args[1..]),
        _ => Err("usage: othello-trainer check|label|fit|inspect|book".into()),
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
    let names: Vec<&str> = "edge corner3 corner2x5 row2 row3 row4 diag8 diag7 diag6 diag5 diag4"
        .split_whitespace()
        .collect();
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

/// `book score|roots|pilot|build <wthor dir> <path> [options]`.
fn book_command(args: &[String]) -> Result<(), String> {
    let usage = "usage: book score|roots|pilot|build <dir> <path> [options]";
    let (command, dir, path) = match args {
        [command, dir, path, ..] => (command.as_str(), PathBuf::from(dir), PathBuf::from(path)),
        _ => return Err(usage.into()),
    };
    let mut params = Params {
        min: 10,
        plies: 30,
        depth: 20,
        table: othello::tt::MAX_ENTRIES,
    };
    let (mut jobs, mut count, mut seed, mut room) = (1, 400, 151, 179_576);
    let (mut depths, mut limit) = (vec![18, 19, 20], 36.0);
    let (mut roots, mut asset, mut manifest) = (None, None, None);
    let mut rest = args[3..].iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let parse = |what: &str| -> Result<usize, String> {
            value.parse().map_err(|_| format!("invalid {what}"))
        };
        match flag.as_str() {
            "--min" => params.min = parse(flag)? as u32,
            "--plies" => params.plies = parse(flag)? as u32,
            "--depth" => params.depth = parse(flag)? as u32,
            "--table" => params.table = parse(flag)?,
            "--jobs" => jobs = parse(flag)?.max(1),
            "--count" => count = parse(flag)?,
            "--seed" => seed = parse(flag)? as u64,
            "--room" => room = parse(flag)?,
            "--limit" => limit = value.parse().map_err(|_| "invalid --limit")?,
            "--depths" => {
                depths = value
                    .split(',')
                    .map(|d| d.parse().map_err(|_| "invalid --depths"))
                    .collect::<Result<_, _>>()?
            }
            "--roots" => roots = Some(PathBuf::from(value)),
            "--asset" => asset = Some(PathBuf::from(value)),
            "--manifest" => manifest = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    let start = std::time::Instant::now();
    let (games, input) = book::read_games(&dir)?;
    let nodes: std::collections::BTreeMap<_, _> = book::count(&games, params.plies)
        .into_iter()
        .filter(|&(_, n)| n >= params.min)
        .collect();
    let write = |path: &PathBuf, bytes: &[u8]| {
        std::fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
    };
    match command {
        "score" => {
            let keys: Vec<_> = nodes.keys().copied().collect();
            let header = book::header(&params, &input);
            let (done, scored) = book::score(&keys, &params, jobs, &path, &header)?;
            println!(
                "nodes {} done {done} scored {scored} elapsed {:.1}s",
                keys.len(),
                start.elapsed().as_secs_f64()
            );
        }
        "roots" => {
            let lines = book::roots(&games, &nodes, 6..=8, count, seed)?;
            let comment = format!(
                "book gate roots: {count} of the movable nodes after 6-8 placements, seed {seed}, min {} input {input}",
                params.min
            );
            write(&path, book::write_lines(&lines, &comment).as_bytes())?;
            println!("roots {} nodes {}", lines.len(), nodes.len());
        }
        "pilot" => {
            let mut sample: Vec<_> = nodes.keys().copied().collect();
            book::shuffle(&mut sample, seed);
            sample.truncate(count);
            let header = format!(
                "{} pilot sample {count} seed {seed} depths {depths:?}",
                book::header(&Params { depth: 0, ..params }, &input)
            );
            let times = book::pilot(&sample, &nodes, &params, &depths, jobs, &path, &header)?;
            let mut bounds = Vec::new();
            for (&depth, times) in &times {
                let (mean, upper) = book::projection(times, nodes.len(), jobs, seed);
                println!(
                    "depth {depth} units {} projected hours mean {mean:.2} upper {upper:.2}",
                    times.len()
                );
                if times.len() == sample.len() {
                    bounds.push((depth, upper));
                }
            }
            println!(
                "nodes {} rule limit {limit} hours: depth {:?}",
                nodes.len(),
                book::depth_rule(&bounds, limit)
            );
        }
        "build" => {
            let path_roots = roots.ok_or("--roots is required")?;
            let text = std::fs::read_to_string(&path_roots)
                .map_err(|error| format!("{}: {error}", path_roots.display()))?;
            let first = book::key(othello::board::Board::START);
            let mut mandatory: std::collections::BTreeSet<_> =
                book::read_roots(&text)?.into_iter().collect();
            mandatory.insert(first);
            let roots: Vec<_> = mandatory
                .iter()
                .flat_map(|&k| [(k, true), (k, false)])
                .collect();
            let (bytes, text) = book::build(&nodes, &path, &roots, &mandatory, room)?;
            write(&asset.ok_or("--asset is required")?, &bytes)?;
            write(&manifest.ok_or("--manifest is required")?, text.as_bytes())?;
            println!(
                "nodes {} records {} bytes {}",
                nodes.len(),
                bytes.len() / 11,
                bytes.len()
            );
        }
        _ => return Err(usage.into()),
    }
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
