use std::io::{self, BufRead, Write};

use crate::agents::{Agent, greedy_agent};
use crate::board::{Board, is_terminal, legal_moves, pass, play, squares};
use crate::eval::evaluate;
use crate::perft::perft;
use crate::search::{SearchConfig, search};

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command] if command == "play" => play_game(),
        [command, depth] if command == "perft" => perft_command(depth),
        [command] if command == "bench" => bench(4),
        [command, flag, depth] if command == "bench" && flag == "--depth" => match depth.parse() {
            Ok(depth) => bench(depth),
            Err(_) => Err("invalid depth".into()),
        },
        _ => Err("usage: othello <play|perft <depth>|bench [--depth N]>".into()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

fn perft_command(depth: &str) -> Result<(), String> {
    match depth.parse::<u32>() {
        Ok(depth) if depth <= 10 => {
            println!("{}", perft(Board::START, depth));
            Ok(())
        }
        _ => Err("invalid depth".into()),
    }
}

fn bench(depth: u32) -> Result<(), String> {
    if depth > 10 {
        return Err("invalid depth".into());
    }
    let config = SearchConfig {
        eval: evaluate,
        endgame_threshold: 12,
    };
    let result = search(Board::START, depth, &config);
    println!(
        "depth {} score {} nodes {}",
        result.depth, result.score, result.nodes
    );
    Ok(())
}

pub fn render(board: Board, black_to_move: bool) -> String {
    let (black, white) = if black_to_move {
        (board.me, board.opp)
    } else {
        (board.opp, board.me)
    };
    let mut out = String::from("  a b c d e f g h\n");
    for rank in (0..8).rev() {
        out.push_str(&format!("{} ", rank + 1));
        for file in 0..8 {
            let bit = 1u64 << (rank * 8 + file);
            out.push_str(if black & bit != 0 {
                "X "
            } else if white & bit != 0 {
                "O "
            } else {
                ". "
            });
        }
        out.push('\n');
    }
    out.push_str(if black_to_move {
        "Black to move"
    } else {
        "White to move"
    });
    out.push_str(&format!(
        ". Black {}, White {}. Legal:",
        black.count_ones(),
        white.count_ones()
    ));
    let mut moves: Vec<String> = squares(legal_moves(board.me, board.opp))
        .map(square_name)
        .collect();
    moves.sort();
    for mv in moves {
        out.push(' ');
        out.push_str(&mv);
    }
    out.push('\n');
    out
}

pub fn parse(text: &str) -> Option<(Board, bool)> {
    let (mut black, mut white, mut rows) = (0u64, 0u64, 0);
    let mut black_to_move = None;
    for line in text.lines() {
        if line.starts_with("Black to move") {
            black_to_move = Some(true);
        } else if line.starts_with("White to move") {
            black_to_move = Some(false);
        } else if let Some((rank, cells)) = parse_row(line) {
            let mut cells = cells.split_whitespace();
            for file in 0..8 {
                let bit = 1u64 << (rank * 8 + file);
                match cells.next()? {
                    "X" => black |= bit,
                    "O" => white |= bit,
                    "." => {}
                    _ => return None,
                }
            }
            if cells.next().is_some() {
                return None;
            }
            rows += 1;
        }
    }
    if rows != 8 {
        return None;
    }
    let black_to_move = black_to_move?;
    let board = if black_to_move {
        Board {
            me: black,
            opp: white,
        }
    } else {
        Board {
            me: white,
            opp: black,
        }
    };
    Some((board, black_to_move))
}

fn parse_row(line: &str) -> Option<(usize, &str)> {
    let bytes = line.as_bytes();
    let rank = *bytes.first()?;
    if !(b'1'..=b'8').contains(&rank) || bytes.get(1) != Some(&b' ') {
        return None;
    }
    Some(((rank - b'1') as usize, &line[2..]))
}

fn square_name(square: u8) -> String {
    let file = (b'a' + square % 8) as char;
    let rank = (b'1' + square / 8) as char;
    format!("{file}{rank}")
}

fn parse_move(board: Board, text: &str) -> Option<u8> {
    let bytes = text.as_bytes();
    if bytes.len() != 2 {
        return None;
    }
    let file = bytes[0].checked_sub(b'a')?;
    let rank = bytes[1].checked_sub(b'1')?;
    if file > 7 || rank > 7 {
        return None;
    }
    let square = rank * 8 + file;
    (legal_moves(board.me, board.opp) & (1u64 << square) != 0).then_some(square)
}

fn play_game() -> Result<(), String> {
    let mut board = Board::START;
    let mut black_to_move = true;
    let mut agent = greedy_agent();
    let stdin = io::stdin();
    let mut input = stdin.lock();
    loop {
        print!("{}", render(board, black_to_move));
        io::stdout().flush().map_err(|error| error.to_string())?;
        if is_terminal(board) {
            let (black, white) = discs(board, black_to_move);
            println!("Game over. Black {black}, White {white}.");
            return Ok(());
        }
        let moves = legal_moves(board.me, board.opp);
        if black_to_move {
            let mut line = String::new();
            if input
                .read_line(&mut line)
                .map_err(|error| error.to_string())?
                == 0
            {
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
        } else if moves == 0 {
            println!("White passes");
            board = pass(board);
        } else {
            let square = agent.choose(board).expect("greedy must move");
            board = play(board, square);
        }
        black_to_move = !black_to_move;
    }
}

fn discs(board: Board, black_to_move: bool) -> (u32, u32) {
    if black_to_move {
        (board.me.count_ones(), board.opp.count_ones())
    } else {
        (board.opp.count_ones(), board.me.count_ones())
    }
}
