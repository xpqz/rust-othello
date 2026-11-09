use std::io::{self, BufRead};
use std::time::Instant;

use crate::agents::{Agent, search_agent};
use crate::board::{Board, is_terminal, legal_moves, pass, play, squares};
use crate::perft::perft;
use crate::search::{NO_DEADLINE, SearchConfig, SearchLimits, probcut_at, search_tt};

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command] if command == "play" => play_game(),
        [command, depth] if command == "perft" => {
            parse_depth(depth, 10).map(|depth| println!("{}", perft(Board::START, depth)))
        }
        [command, rest @ ..] if command == "bench" => bench(rest),
        _ => Err("usage: othello <play|perft <depth>|bench [--depth N]>".into()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

fn parse_depth(text: &str, limit: u32) -> Result<u32, String> {
    match text.parse() {
        Ok(depth) if depth <= limit => Ok(depth),
        _ => Err("invalid depth".into()),
    }
}

fn bench(args: &[String]) -> Result<(), String> {
    let (mut depth, mut time_ms, mut max_nodes) = (10, NO_DEADLINE, 0);
    let (mut board, mut threshold) = (Board::START, SearchConfig::standard().endgame_threshold);
    let mut probcut = None;
    for pair in args.chunks(2) {
        let value = pair.get(1).ok_or("missing bench value")?;
        match pair[0].as_str() {
            "--depth" => depth = parse_depth(value, 64)?,
            "--time-ms" => time_ms = value.parse().map_err(|_| "invalid time")?,
            "--max-nodes" => max_nodes = value.parse().map_err(|_| "invalid nodes")?,
            "--board" => board = parse_board(value)?,
            "--endgame-threshold" => threshold = value.parse().map_err(|_| "invalid threshold")?,
            "--probcut" => {
                probcut = Some(
                    probcut_at(value.parse().map_err(|_| "invalid probcut")?)
                        .map_err(|_| "invalid probcut")?,
                )
            }
            flag => return Err(format!("unknown option: {flag}")),
        }
    }
    let config = SearchConfig {
        endgame_threshold: threshold,
        probcut,
        ..SearchConfig::standard()
    };
    let mut table = crate::tt::Table::new(crate::tt::MAX_ENTRIES);
    let start = Instant::now();
    let limits = SearchLimits::new(depth, time_ms, max_nodes);
    let result = search_tt(board, limits, &config, &mut table);
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    let nps = result.nodes as f64 * 1000.0 / elapsed.max(1e-9);
    let mode = if probcut.is_some() {
        "selective"
    } else {
        "full-width"
    };
    println!(
        "depth {} score {} nodes {} nps {nps:.0} elapsed {elapsed:.2}ms solved {} status {:?} mode {mode} tries {:?} cuts {:?}",
        result.depth,
        result.score,
        result.nodes,
        result.solved as u8,
        result.status,
        result.probcut_tries,
        result.probcut_cuts
    );
    Ok(())
}

fn parse_board(text: &str) -> Result<Board, String> {
    let (me, opp) = text.split_once(':').ok_or("invalid board")?;
    let me = u64::from_str_radix(me, 16).map_err(|_| "invalid board")?;
    let opp = u64::from_str_radix(opp, 16).map_err(|_| "invalid board")?;
    if me & opp != 0 || me | opp == 0 {
        return Err("invalid board".into());
    }
    if legal_moves(me, opp) == 0 {
        return Err("board has no legal move".into());
    }
    Ok(Board { me, opp })
}

fn cell(black: u64, white: u64, bit: u64) -> char {
    match (black & bit != 0, white & bit != 0) {
        (true, _) => 'X',
        (false, true) => 'O',
        _ => '.',
    }
}

fn colours(board: Board, black_to_move: bool) -> (u64, u64) {
    match black_to_move {
        true => (board.me, board.opp),
        false => (board.opp, board.me),
    }
}

pub fn render(board: Board, black_to_move: bool) -> String {
    let (black, white) = colours(board, black_to_move);
    let mut out = String::from(" abcdefgh\n");
    for rank in (0..8).rev() {
        let row: String = (0..8)
            .map(|file| cell(black, white, 1u64 << (rank * 8 + file)))
            .collect();
        out.push_str(&format!("{}{row}\n", rank + 1));
    }
    let legal = squares(legal_moves(board.me, board.opp))
        .map(square_name)
        .collect::<Vec<_>>()
        .join(" ");
    let side = if black_to_move { "Black" } else { "White" };
    let (b, w) = (black.count_ones(), white.count_ones());
    out.push_str(&format!(
        "{side} to move. Black {b}, White {w}, legal: {legal}\n"
    ));
    out
}

pub fn parse(text: &str) -> Option<(Board, bool)> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 10 {
        return None;
    }
    let black_to_move = lines[9].starts_with("Black");
    if !black_to_move && !lines[9].starts_with("White") {
        return None;
    }
    let (mut black, mut white) = (0u64, 0u64);
    for (row, line) in lines[1..9].iter().enumerate() {
        let cells = line.as_bytes();
        if cells.len() != 9 {
            return None;
        }
        for (file, &cell) in cells[1..].iter().enumerate() {
            let bit = 1u64 << ((7 - row) * 8 + file);
            match cell {
                b'X' => black |= bit,
                b'O' => white |= bit,
                b'.' => {}
                _ => return None,
            }
        }
    }
    let (me, opp) = (black, white);
    let (me, opp) = colours(Board { me, opp }, black_to_move);
    Some((Board { me, opp }, black_to_move))
}

fn square_name(square: u8) -> String {
    let file = (b'a' + square % 8) as char;
    let rank = (b'1' + square / 8) as char;
    format!("{file}{rank}")
}

fn parse_move(board: Board, text: &str) -> Option<u8> {
    squares(legal_moves(board.me, board.opp)).find(|&square| square_name(square) == text)
}

fn play_game() -> Result<(), String> {
    let mut board = Board::START;
    let mut black_to_move = true;
    let mut agent = search_agent(SearchConfig::standard(), SearchLimits::new(64, 3000, 0));
    let mut input = io::stdin().lock();
    loop {
        print!("{}", render(board, black_to_move));
        if is_terminal(board) {
            let (black, white) = colours(board, black_to_move);
            let (b, w) = (black.count_ones(), white.count_ones());
            println!("Game over. Black {b}, White {w}.");
            return Ok(());
        }
        let moves = legal_moves(board.me, board.opp);
        if !black_to_move {
            if moves == 0 {
                println!("White passes");
                board = pass(board);
            } else {
                let square = agent.choose(board).expect("search must move");
                board = play(board, square);
            }
        } else if moves == 0 {
            println!("Black passes");
            board = pass(board);
        } else {
            let mut line = String::new();
            if input.read_line(&mut line).unwrap_or(0) == 0 {
                return Ok(());
            }
            let text = line.trim();
            match parse_move(board, text) {
                Some(square) => board = play(board, square),
                _ => {
                    println!("Illegal move: {text}");
                    continue;
                }
            }
        }
        black_to_move = !black_to_move;
    }
}
