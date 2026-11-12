use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use othello::agents::{Agent, greedy_agent, search_agent};
use othello::board::{Board, disc_count, empty_count, final_score, legal_moves, pass, play};
use othello::score::terminal_score;
use othello::search::{NO_DEADLINE, SearchConfig, SearchLimits, search_tt};
use othello::tt::Table;
use othello_tools::journal::Journal;
use othello_tools::positions::{self, Opening};
use othello_tools::provenance::{file_hash, git_commit};
use othello_tools::stats::{RESAMPLES, report_with};

#[derive(Clone, Debug)]
struct Args {
    edax: PathBuf,
    cwd: PathBuf,
    level: u32,
    pairs: usize,
    jobs: usize,
    max_load: f64,
    openings: String,
    out: Option<String>,
    seed: u64,
    time: u64,
    depth: u32,
    bridge: bool,
    ladder: bool,
    attribute: bool,
    levels: String,
}

impl Args {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut a = Self {
            edax: PathBuf::new(),
            cwd: PathBuf::new(),
            level: 1,
            pairs: 50,
            jobs: 4,
            max_load: 6.0,
            openings: concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/openings_tune.txt").into(),
            out: None,
            seed: 1,
            time: 3000,
            depth: 64,
            bridge: false,
            ladder: false,
            attribute: false,
            levels: "10,12".into(),
        };
        let mut timed = false;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--bridge" => {
                    a.bridge = true;
                    index += 1;
                }
                "--ladder" => {
                    a.ladder = true;
                    index += 1;
                }
                "--attribute" => {
                    a.attribute = true;
                    index += 1;
                }
                flag => {
                    let value = args.get(index + 1).ok_or("missing option value")?;
                    match flag {
                        "--edax" => a.edax = value.into(),
                        "--cwd" => a.cwd = value.into(),
                        "--level" => a.level = value.parse().map_err(|_| "invalid level")?,
                        "--pairs" => a.pairs = value.parse().map_err(|_| "invalid pairs")?,
                        "--jobs" => a.jobs = value.parse().map_err(|_| "invalid jobs")?,
                        "--max-load" => {
                            a.max_load = value.parse().map_err(|_| "invalid max-load")?
                        }
                        "--openings" => a.openings = value.clone(),
                        "--out" => a.out = Some(value.clone()),
                        "--seed" => a.seed = value.parse().map_err(|_| "invalid seed")?,
                        "--time" => {
                            a.time = value.parse().map_err(|_| "invalid time")?;
                            timed = true;
                        }
                        "--depth" => a.depth = value.parse().map_err(|_| "invalid depth")?,
                        "--levels" => a.levels = value.clone(),
                        _ => return Err(format!("unknown option: {flag}")),
                    }
                    index += 2;
                }
            }
        }
        if a.edax.as_os_str().is_empty() {
            return Err("missing --edax".into());
        }
        if a.cwd.as_os_str().is_empty() {
            a.cwd = a
                .edax
                .parent()
                .ok_or("edax path has no parent")?
                .to_path_buf();
        }
        if a.depth != 64 {
            if timed {
                return Err("--depth and --time are mutually exclusive".into());
            }
            a.time = NO_DEADLINE;
        }
        if a.jobs == 0 || a.max_load <= 0.0 || a.depth == 0 || a.depth > 64 {
            return Err("invalid jobs, max-load or depth".into());
        }
        Ok(a)
    }
}

fn coord(square: u8) -> String {
    format!(
        "{}{}",
        (b'a' + square % 8) as char,
        (b'1' + square / 8) as char
    )
}

fn square(text: &str) -> Result<u8, String> {
    let bytes = text.as_bytes();
    if bytes.len() != 2 {
        return Err(format!("invalid move {text}"));
    }
    let file = bytes[0]
        .checked_sub(b'a')
        .filter(|file| *file <= 7)
        .ok_or_else(|| format!("invalid move {text}"))?;
    let rank = bytes[1]
        .checked_sub(b'1')
        .filter(|rank| *rank <= 7)
        .ok_or_else(|| format!("invalid move {text}"))?;
    Ok(rank * 8 + file)
}

fn sha256(path: &Path) -> Result<String, String> {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("shasum failed for {}", path.display()));
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or("empty shasum output".into())
}

struct Edax {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
}

impl Edax {
    fn start(args: &Args, level: u32) -> Result<Self, String> {
        let level = level.to_string();
        let mut child = Command::new(&args.edax)
            .current_dir(&args.cwd)
            .args([
                "-gtp",
                "-n",
                "1",
                "-level",
                &level,
                "-book-usage",
                "off",
                "-eval-file",
                "data/eval.dat",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("{}: {error}", args.edax.display()))?;
        let input = child.stdin.take().ok_or("edax stdin")?;
        let output = BufReader::new(child.stdout.take().ok_or("edax stdout")?);
        let mut edax = Self {
            child,
            input,
            output,
            id: 0,
        };
        edax.command("boardsize 8")?;
        edax.command("clear_board")?;
        Ok(edax)
    }

    fn command(&mut self, command: &str) -> Result<Vec<String>, String> {
        self.id += 1;
        writeln!(self.input, "{} {}", self.id, command).map_err(|error| error.to_string())?;
        self.input.flush().map_err(|error| error.to_string())?;
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            if self
                .output
                .read_line(&mut line)
                .map_err(|error| error.to_string())?
                == 0
            {
                return Err("edax closed its output".into());
            }
            let line = line.trim_end_matches(['\r', '\n']).to_string();
            if line.is_empty() {
                break;
            }
            lines.push(line);
        }
        let first = lines.first().ok_or("empty gtp response")?;
        let prefix = format!("= {}", self.id);
        if !first.starts_with(&prefix) {
            return Err(format!("gtp error: {}", lines.join(" | ")));
        }
        let text = first[prefix.len()..].trim().to_string();
        let mut response = Vec::new();
        if !text.is_empty() {
            response.push(text);
        }
        response.extend(lines.into_iter().skip(1));
        Ok(response)
    }

    fn play(&mut self, color: char, square: Option<u8>) -> Result<(), String> {
        let mv = square.map_or_else(|| "ps".into(), coord);
        self.command(&format!("play {color} {mv}"))?;
        Ok(())
    }

    fn genmove(&mut self, color: char) -> Result<Option<u8>, String> {
        let response = self.command(&format!("genmove {color}"))?;
        let text = response.first().map(String::as_str).unwrap_or("");
        if text.eq_ignore_ascii_case("pass") {
            Ok(None)
        } else {
            square(text).map(Some)
        }
    }

    fn discs(&mut self) -> Result<(u32, u32), String> {
        let response = self.command("showboard")?;
        let (mut black, mut white) = (None, None);
        for line in response {
            let line = line.trim();
            let Some(index) = line.find(": discs =") else {
                continue;
            };
            let color = line[..index].trim_end().chars().last();
            let count = line[index + ": discs =".len()..]
                .split_whitespace()
                .next()
                .ok_or("missing disc count")?
                .parse()
                .map_err(|_| "invalid disc count")?;
            if color == Some('*') {
                black = Some(count);
            } else if color == Some('O') {
                white = Some(count);
            }
        }
        Ok((
            black.ok_or("missing black count")?,
            white.ok_or("missing white count")?,
        ))
    }

    fn quit(mut self) -> Result<(), String> {
        self.command("quit")?;
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for Edax {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Clone, Debug)]
struct Game {
    points: f64,
    disc_diff: i32,
    edax_us: u64,
    edax_moves: u32,
    passes: u32,
    black: u32,
    white: u32,
    moves: Vec<Option<u8>>,
}

fn play_game(
    args: &Args,
    opening: &Opening,
    level: u32,
    engine_black: bool,
    engine_time: u64,
    greedy: bool,
) -> Result<Game, String> {
    let mut edax = Edax::start(args, level)?;
    let mut board = Board::START;
    let mut black_to_move = true;
    let mut moves = Vec::new();
    for (ply, &sq) in opening.squares.iter().enumerate() {
        let color = if ply % 2 == 0 { 'b' } else { 'w' };
        edax.play(color, Some(sq))?;
        board = play(board, sq);
        moves.push(Some(sq));
        black_to_move = !black_to_move;
    }
    let mut agent: Box<dyn Agent> = if greedy {
        Box::new(greedy_agent())
    } else {
        Box::new(search_agent(
            SearchConfig::standard(),
            SearchLimits::new(args.depth, engine_time, 0),
        ))
    };
    let mut edax_us = 0;
    let mut edax_moves = 0;
    let mut passes = 0;
    loop {
        let legal = legal_moves(board.me, board.opp);
        if legal == 0 {
            if legal_moves(board.opp, board.me) == 0 {
                break;
            }
            let color = if black_to_move { 'b' } else { 'w' };
            let engine_to_move = black_to_move == engine_black;
            if engine_to_move {
                edax.play(color, None)?;
            } else if edax.genmove(color)?.is_some() {
                return Err("edax returned a move while passing".into());
            }
            board = pass(board);
            moves.push(None);
            passes += 1;
            black_to_move = !black_to_move;
            continue;
        }
        let color = if black_to_move { 'b' } else { 'w' };
        if black_to_move == engine_black {
            let sq = agent.choose(board).ok_or("engine returned no move")?;
            if legal & (1u64 << sq) == 0 {
                return Err(format!("illegal engine move {}", coord(sq)));
            }
            edax.play(color, Some(sq))?;
            board = play(board, sq);
            moves.push(Some(sq));
        } else {
            let start = Instant::now();
            let sq = edax.genmove(color)?.ok_or("edax passed with legal moves")?;
            edax_us += start.elapsed().as_micros() as u64;
            edax_moves += 1;
            if legal & (1u64 << sq) == 0 {
                return Err(format!("illegal edax move {}", coord(sq)));
            }
            board = play(board, sq);
            moves.push(Some(sq));
        }
        black_to_move = !black_to_move;
    }
    let (edax_black, edax_white) = edax.discs()?;
    let (black, white) = if black_to_move {
        (disc_count(board.me), disc_count(board.opp))
    } else {
        (disc_count(board.opp), disc_count(board.me))
    };
    if (black, white) != (edax_black, edax_white) {
        return Err(format!(
            "final count differs: replay {black}-{white}, edax {edax_black}-{edax_white}"
        ));
    }
    let value = terminal_score(final_score(board));
    let value = if engine_black == black_to_move {
        value
    } else {
        -value
    };
    let points = match value.signum() {
        1 => 1.0,
        0 => 0.5,
        _ => 0.0,
    };
    let disc_diff = if engine_black {
        black as i32 - white as i32
    } else {
        white as i32 - black as i32
    };
    edax.quit()?;
    Ok(Game {
        points,
        disc_diff,
        edax_us,
        edax_moves,
        passes,
        black,
        white,
        moves,
    })
}

fn pass_probe(args: &Args) -> Result<(), String> {
    let line = "d3 c3 e6 d6 c6 d2 d1 e3 b4 c1 f3 e1";
    let squares: Vec<u8> = line
        .split_whitespace()
        .map(square)
        .collect::<Result<_, _>>()?;
    let mut edax = Edax::start(args, 1)?;
    for (ply, &sq) in squares.iter().enumerate() {
        let color = if ply % 2 == 0 { 'b' } else { 'w' };
        edax.play(color, Some(sq))?;
    }
    if edax.genmove('b')?.is_some() {
        return Err("pass probe: edax did not pass".into());
    }
    edax.quit()?;
    let mut edax = Edax::start(args, 1)?;
    for (ply, &sq) in squares.iter().enumerate() {
        let color = if ply % 2 == 0 { 'b' } else { 'w' };
        edax.play(color, Some(sq))?;
    }
    edax.play('b', None)?;
    if edax.genmove('w')?.is_none() {
        return Err("pass probe: edax passed with legal moves".into());
    }
    edax.quit()?;
    println!("pass probes: edax pass output and play-pass input agree");
    Ok(())
}

fn bridge(args: &Args) -> Result<(), String> {
    let openings = positions::load(&args.openings)?;
    let count = openings.len().min(10);
    println!(
        "bridge header commit {} date {} edax {} edax_sha256 {} eval_sha256 {}",
        git_commit(),
        utc_date(),
        args.edax.display(),
        sha256(&args.edax)?,
        sha256(&args.cwd.join("data/eval.dat"))?
    );
    let mut passes = 0;
    for (pair, opening) in openings.iter().take(count).enumerate() {
        for game in 0..2 {
            let engine_black = game == 1;
            let result = play_game(args, opening, 1, engine_black, 10, true)?;
            passes += result.passes;
            println!(
                "bridge pair {pair} game {game} engine_black {engine_black} points {:.1} discs {:+} final {}-{} edax_ms {} passes {}",
                result.points,
                result.disc_diff,
                result.black,
                result.white,
                result.edax_us / 1000,
                result.passes
            );
        }
    }
    pass_probe(args)?;
    println!(
        "bridge {}/{} games: final disc counts agree, total passes {}",
        count * 2,
        count * 2,
        passes
    );
    Ok(())
}

#[derive(Clone, Debug)]
struct Saved {
    points: f64,
    void: bool,
}

fn parse_saved(id: &str, line: &str) -> Result<(usize, u64, Saved), String> {
    let (pair, game) = id.split_once(':').ok_or("invalid journal id")?;
    let field = |key: &str| {
        line.split_whitespace()
            .skip_while(|part| *part != key)
            .nth(1)
            .ok_or_else(|| format!("missing {key}"))
    };
    Ok((
        pair.parse().map_err(|_| "invalid pair")?,
        game.parse().map_err(|_| "invalid game")?,
        Saved {
            points: field("points")?.parse().map_err(|_| "invalid points")?,
            void: field("void")? == "1",
        },
    ))
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

fn utc_date() -> String {
    Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map_or_else(|| "unknown".into(), |text| text.trim().to_string())
}

fn header(args: &Args, level: u32) -> Result<String, String> {
    Ok(format!(
        "header v1 level {level} engine {} edax {} edax_sha256 {} eval_sha256 {} commit {} openings {} hash {:016x} jobs {} max_load {:.1}",
        if args.depth == 64 {
            format!("standard time {}", args.time)
        } else {
            format!("standard:d{} time none", args.depth)
        },
        args.edax.display(),
        sha256(&args.edax)?,
        sha256(&args.cwd.join("data/eval.dat"))?,
        git_commit(),
        args.openings,
        file_hash(&args.openings)?,
        args.jobs,
        args.max_load
    ))
}

fn level_path(args: &Args, level: u32) -> String {
    args.out
        .clone()
        .unwrap_or_else(|| format!("docs/tuning/edax_level{level}.journal"))
}

fn run_level_once(args: &Args, openings: &[Opening], level: u32) -> Result<(f64, usize), String> {
    eprintln!("level {level} date {}", utc_date());
    let path = level_path(args, level);
    let (mut journal, _) = Journal::open(Path::new(&path), &header(args, level)?)?;
    let mut saved = BTreeMap::new();
    for record in journal.records().to_vec() {
        if let Ok((pair, game, value)) = parse_saved(&record.id, &record.line) {
            saved.insert((pair, game), value);
        }
    }
    let mut discard = BTreeSet::new();
    let mut queue = Vec::new();
    for pair in 0..args.pairs {
        let left = saved.get(&(pair, 0));
        let right = saved.get(&(pair, 1));
        let valid = matches!((left, right), (Some(a), Some(b)) if !a.void && !b.void);
        if valid {
            continue;
        }
        for game in 0..2 {
            if saved.remove(&(pair, game)).is_some() {
                discard.insert(format!("{pair}:{game}"));
            }
        }
        queue.push(pair);
    }
    journal.discard(&discard)?;
    let journal = Arc::new(Mutex::new(journal));
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    let start = Instant::now();
    thread::scope(|scope| {
        for _ in 0..args.jobs.min(queue.len()) {
            let sender = sender.clone();
            let journal = journal.clone();
            let next = &next;
            let queue = &queue;
            scope.spawn(move || loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(&pair) = queue.get(index) else {
                    break;
                };
                let result = (|| {
                    let mut games = Vec::new();
                    for game in 0..2 {
                        let engine_black = game == 1;
                        games.push(play_game(
                            args,
                            &openings[pair],
                            level,
                            engine_black,
                            args.time,
                            false,
                        )?);
                    }
                    let load = load_average();
                    let void = load > args.max_load;
                    let mut journal = journal.lock().unwrap();
                    for (game, result) in games.iter().enumerate() {
                        let moves = result
                            .moves
                            .iter()
                            .map(|square| square.map_or_else(|| "pass".into(), coord))
                            .collect::<Vec<_>>()
                            .join(" ");
                        let engine_black = game == 1;
                        let payload = format!(
                            "level {level} pair {pair} game {game} engine_black {} points {:.4} discs {:+} edax_us {} edax_moves {} passes {} final {} {} void {} load {load:.3} moves {moves}",
                            u8::from(engine_black),
                            result.points,
                            result.disc_diff,
                            result.edax_us,
                            result.edax_moves,
                            result.passes,
                            result.black,
                            result.white,
                            u8::from(void)
                        );
                        journal.append(&format!("{pair}:{game}"), &payload)?;
                    }
                    if void {
                        let ids = (0..2)
                            .map(|game| format!("{pair}:{game}"))
                            .collect::<BTreeSet<_>>();
                        journal.discard(&ids)?;
                    }
                    Ok::<_, String>((pair, void))
                })();
                if sender.send(result).is_err() {
                    break;
                }
            });
        }
        drop(sender);
        let mut last = Instant::now();
        let mut done = 0;
        loop {
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(Ok((pair, void))) => {
                    done += 1;
                    if void {
                        eprintln!("pair {pair} void above load {}", args.max_load);
                    }
                }
                Ok(Err(error)) => eprintln!("error: {error}"),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if last.elapsed() >= Duration::from_secs(60) {
                eprintln!(
                    "pairs {done}/{} elapsed {:.1}s",
                    queue.len(),
                    start.elapsed().as_secs_f64()
                );
                last = Instant::now();
            }
        }
    });
    let (_, records) = othello_tools::journal::read(Path::new(&path))?;
    let mut per_pair: BTreeMap<usize, [Option<f64>; 2]> = BTreeMap::new();
    let (mut edax_us, mut edax_moves, mut disc_diff, mut games_seen) = (0u64, 0u64, 0i32, 0usize);
    for record in records {
        let (pair, game, value) = parse_saved(&record.id, &record.line)?;
        if value.void {
            continue;
        }
        per_pair.entry(pair).or_default()[game as usize] = Some(value.points);
        let field = |key: &str| -> Result<&str, String> {
            record
                .line
                .split_whitespace()
                .skip_while(|part| *part != key)
                .nth(1)
                .ok_or_else(|| format!("missing {key}"))
        };
        edax_us += field("edax_us")?.parse::<u64>().map_err(|_| "edax_us")?;
        edax_moves += field("edax_moves")?
            .parse::<u64>()
            .map_err(|_| "edax_moves")?;
        disc_diff += field("discs")?.parse::<i32>().map_err(|_| "discs")?;
        games_seen += 1;
    }
    let games: Vec<(f64, f64)> = per_pair
        .values()
        .filter_map(|scores| scores[0].zip(scores[1]))
        .collect();
    let pairs = games.len();
    let report = report_with(&games, RESAMPLES, args.seed);
    let (low, high) = report.interval.bounds().unwrap_or((f64::NAN, f64::NAN));
    let n = games_seen.max(1) as f64;
    println!(
        "level {level} pairs {} games {} W {} D {} L {} mean {:.4} [{:.4}, {:.4}] disc_diff {:.2} edax_ms_per_move {:.2} seed {} resamples {}",
        report.pairs,
        report.games,
        report.wins,
        report.draws,
        report.losses,
        report.mean,
        low,
        high,
        disc_diff as f64 / n,
        edax_us as f64 / edax_moves.max(1) as f64 / 1000.0,
        args.seed,
        RESAMPLES
    );
    eprintln!(
        "level {level} elapsed {:.1}s",
        start.elapsed().as_secs_f64()
    );
    Ok((report.mean, pairs))
}

fn run_level(args: &Args, openings: &[Opening], level: u32) -> Result<f64, String> {
    let mut attempts = 0;
    loop {
        let (mean, pairs) = run_level_once(args, openings, level)?;
        if pairs == args.pairs {
            return Ok(mean);
        }
        attempts += 1;
        if attempts >= 10 {
            return Err(format!(
                "only {pairs}/{} pairs after ten attempts",
                args.pairs
            ));
        }
        eprintln!("replaying {} pairs", args.pairs - pairs);
    }
}

fn field<'a>(line: &'a str, key: &str) -> Result<&'a str, String> {
    line.split_whitespace()
        .skip_while(|part| *part != key)
        .nth(1)
        .ok_or_else(|| format!("missing {key}"))
}

fn parse_attr_moves(payload: &str) -> Result<Vec<Option<u8>>, String> {
    let text = payload.split_once(" moves ").ok_or("missing moves")?.1;
    text.split_whitespace()
        .map(|mv| {
            if mv == "pass" {
                Ok(None)
            } else {
                square(mv).map(Some)
            }
        })
        .collect()
}

fn replay(moves: &[Option<u8>]) -> Result<Vec<(Board, bool)>, String> {
    let mut board = Board::START;
    let mut black = true;
    let mut positions = vec![(board, black)];
    for mv in moves {
        if let Some(sq) = mv {
            if legal_moves(board.me, board.opp) & (1u64 << sq) == 0 {
                return Err(format!("illegal replay move {}", coord(*sq)));
            }
            board = play(board, *sq);
        } else {
            board = pass(board);
        }
        black = !black;
        positions.push((board, black));
    }
    Ok(positions)
}

fn exact_edax(args: &Args, board: Board, black: bool, tag: &str) -> Result<i32, String> {
    let mut text = String::new();
    let (black_bits, white_bits) = if black {
        (board.me, board.opp)
    } else {
        (board.opp, board.me)
    };
    for sq in 0..64 {
        let bit = 1u64 << sq;
        text.push(if black_bits & bit != 0 {
            'X'
        } else if white_bits & bit != 0 {
            'O'
        } else {
            '-'
        });
    }
    text.push(' ');
    text.push(if black { 'X' } else { 'O' });
    text.push_str(";\n");

    let tag = tag.replace(':', "_");
    let path = std::env::temp_dir().join(format!("othello-attr-{}-{tag}.obf", std::process::id()));
    fs::write(&path, text).map_err(|error| error.to_string())?;
    let output = Command::new(&args.edax)
        .current_dir(&args.cwd)
        .args([
            "-solve",
            path.to_str().ok_or("non-utf8 temp path")?,
            "-level",
            "60",
            "-verbose",
            "1",
            "-n",
            "1",
            "-book-usage",
            "off",
            "-eval-file",
            "data/eval.dat",
        ])
        .output()
        .map_err(|error| format!("{}: {error}", args.edax.display()))?;
    let _ = fs::remove_file(&path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let Some((_, rest)) = line.split_once('|') else {
            continue;
        };
        if rest.contains("depth") {
            continue;
        }
        let numbers: Vec<&str> = rest.split_whitespace().collect();
        if numbers.len() < 2 {
            continue;
        }
        // Edax prints `problem|depth score ...`; the second number is the score.
        if let Ok(score) = numbers[1].parse::<i32>() {
            return Ok(score * 100);
        }
    }
    Err(format!(
        "no exact score for {tag}: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn exact_ours(board: Board) -> Result<i32, String> {
    let config = SearchConfig::standard();
    let limits = SearchLimits::new(64, NO_DEADLINE, 0);
    let mut table = Table::new(1 << 20);
    let result = search_tt(board, limits, &config, &mut table);
    Ok(result.score)
}

fn attribute(args: &Args) -> Result<(), String> {
    let levels: Vec<u32> = args
        .levels
        .split(',')
        .map(|text| text.trim().parse().map_err(|_| "invalid levels"))
        .collect::<Result<_, _>>()?;
    let mut total = [[0usize; 3]; 3];
    let mut flipped = [0usize; 3];
    let mut checks = 0;
    let mut checks_ok = 0;

    for level in levels {
        let path = level_path(args, level);
        let text = fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
        let mut matrix = [[0usize; 3]; 3];
        let mut flips = [0usize; 3];
        let (mut big, mut mid, mut drawn, mut won) = (0, 0, 0, 0);

        for line in text.lines().skip(1) {
            let Some((id, payload)) = line.split_once('\t') else {
                continue;
            };
            let points: f64 = field(payload, "points")?
                .parse()
                .map_err(|_| "invalid points")?;
            let actual = if points == 1.0 {
                0
            } else if points == 0.5 {
                1
            } else {
                2
            };
            let engine_black = field(payload, "engine_black")? == "1";
            let moves = parse_attr_moves(payload)?;
            let positions = replay(&moves)?;
            let Some((index, &(board, black))) = positions
                .iter()
                .enumerate()
                .find(|(_, (board, _))| empty_count(*board) <= 24)
            else {
                return Err(format!("{level} {id}: no position at 24 empties"));
            };
            let value = exact_edax(args, board, black, &format!("{level}-{id}-24"))?;
            let value = if engine_black == black { value } else { -value };
            let state = if value > 0 {
                0
            } else if value == 0 {
                1
            } else {
                2
            };
            matrix[actual][state] += 1;
            if actual != state {
                flips[actual] += 1;
            }
            if actual == 2 {
                if value <= -1000 {
                    big += 1;
                } else if value < 0 {
                    mid += 1;
                } else if value == 0 {
                    drawn += 1;
                } else {
                    won += 1;
                }
            }
            println!(
                "level {level} {id} result {} engine_black {} exact24 {:+} state {}",
                if actual == 0 {
                    "win"
                } else if actual == 1 {
                    "draw"
                } else {
                    "loss"
                },
                u8::from(engine_black),
                value,
                if state == 0 {
                    "won"
                } else if state == 1 {
                    "drawn"
                } else {
                    "lost"
                }
            );

            if checks < 5 {
                let later = positions
                    .iter()
                    .enumerate()
                    .skip(index + 1)
                    .find(|(_, (board, _))| empty_count(*board) <= 14);
                if let Some((later_index, &(later_board, later_black))) = later {
                    let edax = exact_edax(
                        args,
                        later_board,
                        later_black,
                        &format!("{level}-{id}-check-{later_index}"),
                    )?;
                    let ours = exact_ours(later_board)?;
                    checks += 1;
                    if edax == ours {
                        checks_ok += 1;
                    }
                    println!(
                        "check level {level} {id} empties {} engine_black {} edax {:+} ours {:+} {}",
                        empty_count(later_board),
                        u8::from(engine_black),
                        edax,
                        ours,
                        if edax == ours { "agree" } else { "DIFFER" }
                    );
                }
            }
        }

        println!(
            "level {level} actual wins {} (won24 {}, drawn24 {}, lost24 {}); draws {} (won24 {}, drawn24 {}, lost24 {}); losses {} (won24 {}, drawn24 {}, lost24 {})",
            matrix[0][0] + matrix[0][1] + matrix[0][2],
            matrix[0][0],
            matrix[0][1],
            matrix[0][2],
            matrix[1][0] + matrix[1][1] + matrix[1][2],
            matrix[1][0],
            matrix[1][1],
            matrix[1][2],
            matrix[2][0] + matrix[2][1] + matrix[2][2],
            matrix[2][0],
            matrix[2][1],
            matrix[2][2]
        );
        println!(
            "level {level} flipped_to_win {} flipped_to_draw {} flipped_to_loss {}",
            flips[0], flips[1], flips[2]
        );
        println!("level {level} losses: by10plus {big}, by2to8 {mid}, drawn {drawn}, won {won}");
        for actual in 0..3 {
            for state in 0..3 {
                total[actual][state] += matrix[actual][state];
            }
            flipped[actual] += flips[actual];
        }
    }
    println!(
        "attribution total: wins {} (won24 {}, drawn24 {}, lost24 {}); draws {} (won24 {}, drawn24 {}, lost24 {}); losses {} (won24 {}, drawn24 {}, lost24 {})",
        total[0][0] + total[0][1] + total[0][2],
        total[0][0],
        total[0][1],
        total[0][2],
        total[1][0] + total[1][1] + total[1][2],
        total[1][0],
        total[1][1],
        total[1][2],
        total[2][0] + total[2][1] + total[2][2],
        total[2][0],
        total[2][1],
        total[2][2]
    );
    println!(
        "attribution flips: to_win {}, to_draw {}, to_loss {}; spot_checks {checks_ok}/{checks}",
        flipped[0], flipped[1], flipped[2]
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = Args::parse(&args).and_then(|args| {
        if args.bridge {
            return bridge(&args);
        }
        if args.attribute {
            return attribute(&args);
        }
        let openings = positions::load(&args.openings)?;
        if args.pairs > openings.len() {
            return Err("pairs exceed openings".into());
        }
        if args.ladder {
            for level in [1, 2, 4, 6, 8, 10, 12] {
                if run_level(&args, &openings, level)? < 0.25 {
                    println!("stopped after level {level}");
                    break;
                }
            }
            Ok(())
        } else {
            run_level(&args, &openings, args.level).map(|_| ())
        }
    });
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}
