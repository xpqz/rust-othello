use std::path::Path;

use othello::board::{Board, is_terminal, legal_moves, pass, play};

pub const HEADER: usize = 16;
pub const RECORD: usize = 68;
const INVALID: u8 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub games: usize,
    pub year: u16,
    pub p3: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    pub tournament: u16,
    pub black: u16,
    pub white: u16,
    pub real: u8,
    pub theoretical: u8,
    pub moves: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Database {
    pub header: Header,
    pub games: Vec<Game>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    pub positions: Vec<Board>,
    pub black: Vec<bool>,
    pub final_board: Board,
    pub final_black: bool,
    pub passes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    Illegal { ply: usize },
    Incomplete,
}

pub fn read(bytes: &[u8]) -> Result<Database, String> {
    if bytes.len() < HEADER {
        return Err("file shorter than header".into());
    }
    let games = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let year = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
    let p1 = bytes[12];
    let p2 = bytes[13];
    let p3 = if bytes[14] == 0 { 22 } else { bytes[14] };
    if p1 != 0 && p1 != 8 {
        return Err(format!("unsupported board size {p1}"));
    }
    if p2 != 0 {
        return Err(format!("unsupported file type {p2}"));
    }
    let expected = HEADER + RECORD * games;
    if bytes.len() != expected {
        return Err(format!("file length {} != {expected}", bytes.len()));
    }
    let mut parsed = Vec::with_capacity(games);
    for index in 0..games {
        let record = &bytes[HEADER + RECORD * index..HEADER + RECORD * (index + 1)];
        let moves = record[8..]
            .iter()
            .take_while(|&&byte| byte != 0)
            .map(|&byte| decode(byte).unwrap_or(INVALID))
            .collect();
        parsed.push(Game {
            tournament: u16::from_le_bytes(record[0..2].try_into().unwrap()),
            black: u16::from_le_bytes(record[2..4].try_into().unwrap()),
            white: u16::from_le_bytes(record[4..6].try_into().unwrap()),
            real: record[6],
            theoretical: record[7],
            moves,
        });
    }
    Ok(Database {
        header: Header { games, year, p3 },
        games: parsed,
    })
}

pub fn read_path(path: &Path) -> Result<Database, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    read(&bytes)
}

pub fn replay(game: &Game) -> Result<Replay, Reject> {
    let mut board = Board::START;
    let mut positions = Vec::new();
    let mut blacks = Vec::new();
    let mut black = true;
    let mut passes = 0;
    for (ply, &square) in game.moves.iter().enumerate() {
        while legal_moves(board.me, board.opp) == 0 {
            if legal_moves(board.opp, board.me) == 0 {
                return Err(Reject::Illegal { ply });
            }
            passes += 1;
            black = !black;
            board = pass(board);
        }
        let legal = legal_moves(board.me, board.opp);
        if square >= 64 || legal & (1u64 << square) == 0 {
            return Err(Reject::Illegal { ply });
        }
        positions.push(board);
        blacks.push(black);
        black = !black;
        board = play(board, square);
    }
    while !is_terminal(board) {
        if legal_moves(board.me, board.opp) != 0 {
            return Err(Reject::Incomplete);
        }
        passes += 1;
        black = !black;
        board = pass(board);
    }
    Ok(Replay {
        positions,
        black: blacks,
        final_board: board,
        final_black: black,
        passes,
    })
}

fn decode(byte: u8) -> Option<u8> {
    let column = byte % 10;
    let row = byte / 10;
    if (1..=8).contains(&column) && (1..=8).contains(&row) {
        Some((row - 1) * 8 + column - 1)
    } else {
        None
    }
}
