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

/// `book score|roots|pilot|build <wthor dir> <path> [options]`. Every
/// option is checked before any input is read or output written.
fn book_command(args: &[String]) -> Result<(), String> {
    let usage = "usage: book score|roots|pilot|build <dir> <path> [options]";
    let [command, dir, path, rest @ ..] = args else {
        return Err(usage.into());
    };
    let allowed = match command.as_str() {
        "score" => "--min --plies --depth --table --jobs",
        "roots" => "--min --plies --count --seed",
        "pilot" => "--min --plies --table --jobs --sample --seed --depths --threads --limit",
        "build" => "--min --plies --roots --asset --manifest --room",
        _ => return Err(usage.into()),
    };
    let mut given = std::collections::BTreeMap::new();
    for pair in rest.chunks(2) {
        let [flag, value] = pair else {
            return Err(format!("{} needs a value", pair[0]));
        };
        if !allowed.split(' ').any(|known| known == flag) {
            return Err(format!("option {flag} not valid for book {command}"));
        }
        if given.insert(flag.as_str(), value.as_str()).is_some() {
            return Err(format!("repeated option {flag}"));
        }
    }
    let get =
        |flag: &str, default: u64, range: std::ops::RangeInclusive<u64>| -> Result<u64, String> {
            given.get(flag).map_or(Ok(default), |text| {
                text.parse()
                    .ok()
                    .filter(|n| range.contains(n))
                    .ok_or_else(|| format!("invalid {flag} {text}"))
            })
        };
    let table = othello::tt::MAX_ENTRIES as u64;
    let params = Params {
        min: get("--min", 10, 1..=u32::MAX as u64)? as u32,
        plies: get("--plies", 30, 1..=61)? as u32,
        depth: get("--depth", 20, 1..=60)? as u32,
        table: get("--table", table, 1..=table)? as usize,
    };
    let jobs = get("--jobs", 1, 1..=256)? as usize;
    let threads = get("--threads", 5, 1..=256)? as usize;
    let pilot = command == "pilot";
    let (sample, defaults) = if pilot {
        ("--sample", (200, 150))
    } else {
        ("--count", (400, 151))
    };
    let count = get(sample, defaults.0, 1..=u32::MAX as u64)? as usize;
    let seed = get("--seed", defaults.1, 0..=u64::MAX)?;
    let room = get("--room", 179_576, 11..=4 << 20)? as usize;
    let limit: f64 = given.get("--limit").map_or(Ok(36.0), |text| {
        text.parse()
            .ok()
            .filter(|l: &f64| l.is_finite() && *l > 0.0)
            .ok_or(format!("invalid --limit {text}"))
    })?;
    let mut depths = vec![18, 19, 20];
    if let Some(text) = given.get("--depths") {
        depths = text
            .split(',')
            .map(|d| d.parse().ok().filter(|d| (1..=60).contains(d)))
            .collect::<Option<Vec<u32>>>()
            .ok_or(format!("invalid --depths {text}"))?;
        if depths.windows(2).any(|w| w[0] >= w[1]) {
            return Err(format!("--depths must be strictly increasing: {text}"));
        }
    }
    let output = |flag: &str| {
        given
            .get(flag)
            .map(PathBuf::from)
            .ok_or(format!("{flag} is required"))
    };
    let outputs = if command == "build" {
        Some((
            output("--roots")?,
            output("--asset")?,
            output("--manifest")?,
        ))
    } else {
        None
    };
    let (path, start) = (PathBuf::from(path), std::time::Instant::now());
    let (games, data) = book::read_games(&PathBuf::from(dir))?;
    let rejected = games.iter().filter(|game| !book::legal_game(game)).count();
    let nodes: std::collections::BTreeMap<_, _> = book::count(&games, params.plies)
        .into_iter()
        .filter(|&(_, n)| n >= params.min)
        .collect();
    let input = book::input(&data, &nodes);
    println!(
        "games {} rejected {rejected} nodes {}",
        games.len(),
        nodes.len()
    );
    match command.as_str() {
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
                "book gate roots: {count} movable nodes after 6-8 placements, seed {seed}, min {} input {input}",
                params.min
            );
            std::fs::write(&path, book::write_lines(&lines, &comment))
                .map_err(|error| format!("{}: {error}", path.display()))?;
            println!("roots {}", lines.len());
        }
        "pilot" => {
            let mut sample: Vec<_> = nodes.keys().copied().collect();
            book::shuffle(&mut sample, seed);
            sample.truncate(count);
            let base = book::header(&Params { depth: 1, ..params }, &input);
            let header = format!(
                "{base} pilot sample {count} seed {seed} depths {depths:?} threads {threads} limit {limit}"
            );
            let times = book::pilot(&sample, &nodes, &params, &depths, jobs, &path, &header)?;
            let mut bounds = Vec::new();
            for (&depth, times) in &times {
                let (mean, upper) = book::projection(times, nodes.len(), threads, seed);
                println!(
                    "depth {depth} units {} hours on {threads} threads mean {mean:.4} upper {upper:.4}",
                    times.len()
                );
                bounds.push((depth, upper));
            }
            println!(
                "rule limit {limit} hours: depth {:?}",
                book::depth_rule(&bounds, limit)
            );
        }
        _ => {
            let (roots, asset, manifest) = outputs.ok_or(usage)?;
            let text = std::fs::read_to_string(&roots)
                .map_err(|error| format!("{}: {error}", roots.display()))?;
            let mut mandatory: std::collections::BTreeSet<_> =
                book::read_roots(&text)?.into_iter().collect();
            mandatory.insert(book::key(othello::board::Board::START));
            let walk: Vec<_> = mandatory
                .iter()
                .flat_map(|&k| [(k, true), (k, false)])
                .collect();
            let identity = book::identity(&params, &input);
            let (bytes, text) = book::build(&nodes, &path, &identity, &walk, &mandatory, room)?;
            for (file, contents) in [(&asset, &bytes[..]), (&manifest, text.as_bytes())] {
                std::fs::write(file, contents)
                    .map_err(|error| format!("{}: {error}", file.display()))?;
            }
            println!("records {} bytes {}", bytes.len() / 11, bytes.len());
        }
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
