//! Paired match between two builds of the engine, each run as an
//! `engine_worker` process: one process per side per game, same time per move.
//! Resumable: one journal record per game; pairs played above the load limit
//! are discarded and replayed.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use othello::board::{Board, final_score, is_terminal, legal_moves, pass, play};
use othello_tools::journal::Journal;
use othello_tools::positions::{self, Opening};
use othello_tools::provenance::{file_hash, git_commit};
use othello_tools::stats::{RESAMPLES, report_with};

struct Args {
    a: PathBuf,
    b: PathBuf,
    time: u64,
    pairs: usize,
    jobs: usize,
    max_load: f64,
    seed: u64,
    progress: u64,
    openings: String,
    out: String,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        a: PathBuf::new(),
        b: PathBuf::new(),
        time: 3000,
        pairs: 100,
        jobs: 4,
        max_load: 6.0,
        seed: 1,
        progress: 60,
        openings: concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/openings_tune.txt").into(),
        out: String::new(),
    };
    for pair in args.chunks(2) {
        let [flag, value] = pair else {
            return Err("missing option value".into());
        };
        let bad = || format!("invalid {flag}");
        match flag.as_str() {
            "--a" => a.a = value.into(),
            "--b" => a.b = value.into(),
            "--time" => a.time = value.parse().map_err(|_| bad())?,
            "--pairs" => a.pairs = value.parse().map_err(|_| bad())?,
            "--jobs" => a.jobs = value.parse().map_err(|_| bad())?,
            "--max-load" => a.max_load = value.parse().map_err(|_| bad())?,
            "--seed" => a.seed = value.parse().map_err(|_| bad())?,
            "--progress" => a.progress = value.parse().map_err(|_| bad())?,
            "--openings" => a.openings = value.clone(),
            "--out" => a.out = value.clone(),
            _ => return Err(format!("unknown option: {flag}")),
        }
    }
    if a.a.as_os_str().is_empty() || a.b.as_os_str().is_empty() || a.out.is_empty() {
        return Err("usage: version_match --a WORKER --b WORKER --out JOURNAL [options]".into());
    }
    if a.jobs == 0 || a.max_load <= 0.0 || a.progress == 0 {
        return Err("invalid jobs or max-load".into());
    }
    Ok(a)
}

struct Worker {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Worker {
    fn start(path: &Path, time: u64) -> Result<Self, String> {
        let mut child = Command::new(path)
            .args(["--time", &time.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let input = child.stdin.take().ok_or("no worker stdin")?;
        let output = BufReader::new(child.stdout.take().ok_or("no worker stdout")?);
        Ok(Self {
            child,
            input,
            output,
        })
    }

    fn choose(&mut self, board: Board) -> Result<u8, String> {
        writeln!(self.input, "go {:016x} {:016x}", board.me, board.opp)
            .map_err(|error| error.to_string())?;
        let mut line = String::new();
        self.output
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        let square: u8 = line
            .trim()
            .parse()
            .map_err(|_| format!("bad worker reply: {line:?}"))?;
        if square >= 64 || legal_moves(board.me, board.opp) >> square & 1 == 0 {
            return Err(format!("illegal worker move {square}"));
        }
        Ok(square)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Plays one game; returns A's points and A's final disc margin (WTHOR).
fn play_game(args: &Args, opening: &Opening, a_black: bool) -> Result<(f64, i32, String), String> {
    let mut a = Worker::start(&args.a, args.time)?;
    let mut b = Worker::start(&args.b, args.time)?;
    let mut board = Board::START;
    let mut black = true;
    let mut moves = Vec::new();
    for &square in &opening.squares {
        board = play(board, square);
        black = !black;
        moves.push(positions::square_name(square));
    }
    while !is_terminal(board) {
        if legal_moves(board.me, board.opp) == 0 {
            board = pass(board);
            moves.push("pass".into());
        } else {
            let mover = if black == a_black { &mut a } else { &mut b };
            let square = mover.choose(board)?;
            board = play(board, square);
            moves.push(positions::square_name(square));
        }
        black = !black;
    }
    let mover_margin = final_score(board);
    let margin = if black == a_black {
        mover_margin
    } else {
        -mover_margin
    };
    let points = match margin.signum() {
        1 => 1.0,
        0 => 0.5,
        _ => 0.0,
    };
    Ok((points, margin, moves.join(" ")))
}

fn load_average() -> f64 {
    Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .ok()
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .find_map(|token| token.trim_matches(['{', '}']).parse().ok())
        })
        .unwrap_or(0.0)
}

fn number<T: std::str::FromStr>(line: &str, key: &str) -> Result<T, String> {
    line.split_whitespace()
        .skip_while(|part| *part != key)
        .nth(1)
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| format!("missing {key}"))
}

fn sha256(path: &Path) -> Result<String, String> {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout);
    match text.split_whitespace().next() {
        Some(hash) if output.status.success() => Ok(hash.to_string()),
        _ => Err(format!("shasum failed for {}", path.display())),
    }
}

/// A's points and disc margin by (pair, game).
type Games = BTreeMap<(usize, usize), (f64, i32)>;

fn valid(journal: &Journal) -> Result<Games, String> {
    let mut games = BTreeMap::new();
    for record in journal.records() {
        let (pair, game) = record.id.split_once(':').ok_or("invalid journal id")?;
        let key = (
            pair.parse().map_err(|_| "invalid pair")?,
            game.parse().map_err(|_| "invalid game")?,
        );
        games.insert(
            key,
            (
                number(&record.line, "points")?,
                number(&record.line, "discs")?,
            ),
        );
    }
    Ok(games)
}

/// Plays the pairs still missing; a pair finished above the load limit is not
/// recorded. Returns the number of complete pairs.
fn play_missing(args: &Args, openings: &[Opening], journal: &mut Journal) -> Result<usize, String> {
    let games = valid(journal)?;
    let complete =
        |pair: &usize| games.contains_key(&(*pair, 0)) && games.contains_key(&(*pair, 1));
    let partial: BTreeSet<String> = games
        .keys()
        .filter(|(pair, _)| !complete(pair))
        .map(|(pair, game)| format!("{pair}:{game}"))
        .collect();
    journal.discard(&partial)?;
    let queue: Vec<usize> = (0..args.pairs).filter(|pair| !complete(pair)).collect();
    let next = AtomicUsize::new(0);
    let journal = Mutex::new(journal);
    let (sender, receiver) = mpsc::channel();
    let start = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..args.jobs.min(queue.len()) {
            let sender = sender.clone();
            let (next, queue, journal) = (&next, &queue, &journal);
            scope.spawn(move || {
                while let Some(&pair) = queue.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let result = (|| {
                        let first = play_game(args, &openings[pair], true)?;
                        let second = play_game(args, &openings[pair], false)?;
                        let load = load_average();
                        if load > args.max_load {
                            eprintln!("pair {pair} void above load {}", args.max_load);
                            return Ok(false);
                        }
                        let mut journal = journal.lock().unwrap();
                        for (game, (points, discs, moves)) in [first, second].iter().enumerate() {
                            journal.append(
                                &format!("{pair}:{game}"),
                                &format!(
                                    "pair {pair} game {game} a_black {} points {points:.4} discs {discs:+} load {load:.3} moves {moves}",
                                    u8::from(game == 0)
                                ),
                            )?;
                        }
                        Ok::<_, String>(true)
                    })();
                    if sender.send(result).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        // Reported at least every `progress` seconds, whether or not a pair
        // has finished: complete pairs (earlier runs included), elapsed, ETA.
        let before = args.pairs - queue.len();
        let (mut played, mut kept) = (0, 0);
        let mut last = Instant::now();
        loop {
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(Ok(valid)) => {
                    played += 1;
                    kept += usize::from(valid);
                }
                Ok(Err(error)) => eprintln!("error: {error}"),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if last.elapsed() >= Duration::from_secs(args.progress) {
                last = Instant::now();
                let elapsed = start.elapsed().as_secs_f64();
                let eta = match played {
                    0 => "unknown".to_string(),
                    _ => format!(
                        "{:.0}s",
                        elapsed / played as f64 * (queue.len() - played) as f64
                    ),
                };
                eprintln!(
                    "progress {}/{} pairs valid, {played}/{} played this pass, elapsed {elapsed:.0}s eta {eta}",
                    before + kept,
                    args.pairs,
                    queue.len()
                );
            }
        }
    });
    let journal = journal.into_inner().unwrap();
    Ok(valid(journal)?.len() / 2)
}

fn run(args: &Args) -> Result<(), String> {
    let openings = positions::load(&args.openings)?;
    if args.pairs > openings.len() {
        return Err("pairs exceed openings".into());
    }
    let header = format!(
        "header v1 versions a {} a_sha256 {} b {} b_sha256 {} time {} pairs {} seed {} openings {} hash {:016x} jobs {} max_load {} driver {}",
        args.a.display(),
        sha256(&args.a)?,
        args.b.display(),
        sha256(&args.b)?,
        args.time,
        args.pairs,
        args.seed,
        args.openings,
        file_hash(&args.openings)?,
        args.jobs,
        args.max_load,
        git_commit()
    );
    let (mut journal, _) = Journal::open(Path::new(&args.out), &header)?;
    let mut attempts = 0;
    while play_missing(args, &openings, &mut journal)? < args.pairs {
        attempts += 1;
        if attempts >= 10 {
            return Err(format!("incomplete after ten attempts: {}", args.out));
        }
        eprintln!("replaying missing pairs");
    }
    let games = valid(&journal)?;
    let pairs: Vec<_> = (0..args.pairs)
        .map(|pair| (games[&(pair, 0)], games[&(pair, 1)]))
        .collect();
    let points: Vec<(f64, f64)> = pairs.iter().map(|(x, y)| (x.0, y.0)).collect();
    let report = report_with(&points, RESAMPLES, args.seed);
    let (low, high) = report.interval.bounds().unwrap_or((f64::NAN, f64::NAN));
    let margins: Vec<f64> = pairs
        .iter()
        .map(|(x, y)| f64::from(x.1 + y.1) / 2.0)
        .collect();
    let n = margins.len() as f64;
    let mean = margins.iter().sum::<f64>() / n;
    let variance = margins.iter().map(|m| (m - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    println!(
        "a_vs_b pairs {} games {} W {} D {} L {} mean {:.4} [{:.4}, {:.4}] informative {} discs {mean:+.2} pair_se {:.2} seed {} resamples {RESAMPLES}",
        report.pairs,
        report.games,
        report.wins,
        report.draws,
        report.losses,
        report.mean,
        low,
        high,
        report.interval.informative(),
        (variance / n).sqrt(),
        args.seed
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(message) = parse_args(&args).and_then(|args| run(&args)) {
        eprintln!("{message}");
        std::process::exit(2);
    }
}
