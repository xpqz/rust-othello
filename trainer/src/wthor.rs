use othello::board::{Board, is_terminal, legal_moves, pass, play};

pub const HEADER: usize = 16;
pub const RECORD: usize = 68;

pub struct Game {
    pub real: u8,
    pub theoretical: u8,
    pub moves: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Replay {
    pub positions: Vec<(Board, bool)>,
    pub final_board: Board,
    pub final_black: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    Illegal { ply: usize },
    Incomplete,
}

pub fn read(bytes: &[u8]) -> Result<(u8, Vec<Game>), String> {
    if bytes.len() < HEADER {
        return Err("file shorter than header".into());
    }
    let games = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let p1 = bytes[12];
    let p2 = bytes[13];
    if (p1 != 0 && p1 != 8) || p2 != 0 {
        return Err(format!("unsupported board {p1} or file type {p2}"));
    }
    let expected = HEADER + RECORD * games;
    if bytes.len() != expected {
        return Err(format!("file length {} != {expected}", bytes.len()));
    }
    let records = bytes[HEADER..]
        .as_chunks::<RECORD>()
        .0
        .iter()
        .map(|record| Game {
            real: record[6],
            theoretical: record[7],
            moves: record[8..]
                .iter()
                .take_while(|&&byte| byte != 0)
                .map(|&byte| decode(byte).unwrap_or(u8::MAX))
                .collect(),
        })
        .collect();
    Ok((if bytes[14] == 0 { 22 } else { bytes[14] }, records))
}

pub fn replay(game: &Game) -> Result<Replay, Reject> {
    let mut board = Board::START;
    let mut positions = Vec::new();
    let mut black = true;
    for (ply, &square) in game.moves.iter().enumerate() {
        while legal_moves(board.me, board.opp) == 0 {
            if legal_moves(board.opp, board.me) == 0 {
                return Err(Reject::Illegal { ply });
            }
            black = !black;
            board = pass(board);
        }
        let legal = legal_moves(board.me, board.opp);
        if square >= 64 || legal & (1u64 << square) == 0 {
            return Err(Reject::Illegal { ply });
        }
        positions.push((board, black));
        black = !black;
        board = play(board, square);
    }
    while !is_terminal(board) {
        if legal_moves(board.me, board.opp) != 0 {
            return Err(Reject::Incomplete);
        }
        black = !black;
        board = pass(board);
    }
    Ok(Replay {
        positions,
        final_board: board,
        final_black: black,
    })
}

fn decode(byte: u8) -> Option<u8> {
    let (column, row) = (byte % 10, byte / 10);
    ((1..=8).contains(&column) && (1..=8).contains(&row)).then(|| (row - 1) * 8 + column - 1)
}
