use std::io::{self, BufRead};

use crate::agents::{Agent, greedy_agent};
use crate::board::{Board, is_terminal, legal_moves, pass, play, squares};
use crate::eval::evaluate;
use crate::perft::perft;
use crate::search::{SearchConfig, search};

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command] if command == "play" => play_game(),
        [command, depth] if command == "perft" => {
            parse_depth(depth).map(|depth| println!("{}", perft(Board::START, depth)))
        }
        [command] if command == "bench" => bench(4),
        [command, flag, depth] if command == "bench" && flag == "--depth" => {
            parse_depth(depth).and_then(bench)
        }
        _ => Err("usage: othello <play|perft <depth>|bench [--depth N]>".into()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

fn parse_depth(text: &str) -> Result<u32, String> {
    match text.parse() {
        Ok(depth) if depth <= 10 => Ok(depth),
        _ => Err("invalid depth".into()),
    }
}

fn bench(depth: u32) -> Result<(), String> {
    let config = SearchConfig {
        eval: evaluate,
        endgame_threshold: 12,
    };
    let result = search(Board::START, depth, &config);
    let (d, s, n) = (result.depth, result.score, result.nodes);
    println!("depth {d} score {s} nodes {n}");
    Ok(())
}

fn colours(board: Board, black_to_move: bool) -> (u64, u64) {
    if black_to_move {
        (board.me, board.opp)
    } else {
        (board.opp, board.me)
    }
}

pub fn render(board: Board, black_to_move: bool) -> String {
    let (black, white) = colours(board, black_to_move);
    let mut out = String::new();
    for rank in (0..8).rev() {
        out.push((b'1' + rank as u8) as char);
        for file in 0..8 {
            let bit = 1u64 << (rank * 8 + file);
            let cell = if black & bit != 0 {
                'X'
            } else if white & bit != 0 {
                'O'
            } else {
                '.'
            };
            out.push(cell);
        }
        out.push('\n');
    }
    let legal: Vec<_> = squares(legal_moves(board.me, board.opp))
        .map(square_name)
        .collect();
    let legal = legal.join(" ");
    let side = if black_to_move { "Black" } else { "White" };
    let (b, w) = (black.count_ones(), white.count_ones());
    out.push_str(&format!(
        "{side} to move. Black {b}, White {w}, legal: {legal}\n"
    ));
    out
}

pub fn parse(text: &str) -> Option<(Board, bool)> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 9 {
        return None;
    }
    if !lines[8].starts_with("Black") && !lines[8].starts_with("White") {
        return None;
    }
    let black_to_move = lines[8].starts_with("Black");
    let (mut black, mut white) = (0u64, 0u64);
    for (row, line) in lines[..8].iter().enumerate() {
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
    let (me, opp) = if black_to_move { (me, opp) } else { (opp, me) };
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
    let mut agent = greedy_agent();
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
                let square = agent.choose(board).expect("greedy must move");
                board = play(board, square);
            }
        } else {
            let mut line = String::new();
            if input.read_line(&mut line).unwrap_or(0) == 0 {
                return Ok(());
            }
            let text = line.trim();
            match parse_move(board, text) {
                Some(square) => board = play(board, square),
                None if text == "pass" && moves == 0 => {
                    println!("Black passes");
                    board = pass(board);
                }
                _ => {
                    println!("Illegal move: {text}");
                    continue;
                }
            }
        }
        black_to_move = !black_to_move;
    }
}
