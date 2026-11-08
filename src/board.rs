//! Bitboards; square and start conventions are in the crate docs.

const NOT_A: u64 = 0xfefe_fefe_fefe_fefe;
const NOT_H: u64 = 0x7f7f_7f7f_7f7f_7f7f;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dir(i8, u64);

impl Dir {
    pub const E: Dir = Dir(1, NOT_A);
    pub const W: Dir = Dir(-1, NOT_H);
    pub const N: Dir = Dir(8, !0);
    pub const S: Dir = Dir(-8, !0);
    pub const NE: Dir = Dir(9, NOT_A);
    pub const NW: Dir = Dir(7, NOT_H);
    pub const SE: Dir = Dir(-7, NOT_A);
    pub const SW: Dir = Dir(-9, NOT_H);
    pub const ALL: [Dir; 8] = [
        Dir::E,
        Dir::W,
        Dir::N,
        Dir::S,
        Dir::NE,
        Dir::NW,
        Dir::SE,
        Dir::SW,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Board {
    pub me: u64,
    pub opp: u64,
}

impl Board {
    /// Black on d5/e4, white on d4/e5, black to move.
    pub const START: Board = Board {
        me: (1 << 28) | (1 << 35),
        opp: (1 << 27) | (1 << 36),
    };
}

pub fn shift(bits: u64, Dir(step, mask): Dir) -> u64 {
    if step > 0 {
        (bits << step) & mask
    } else {
        (bits >> -step) & mask
    }
}

pub fn legal_moves(me: u64, opp: u64) -> u64 {
    let empty = !(me | opp);
    let mut moves = 0;
    for dir in Dir::ALL {
        let mut run = shift(me, dir) & opp;
        let mut line = 0;
        while run != 0 {
            line |= run;
            run = shift(run, dir) & opp;
        }
        moves |= shift(line, dir) & empty;
    }
    moves
}

pub fn flips(board: Board, sq: u8) -> u64 {
    let mut flipped = 0;
    for dir in Dir::ALL {
        let mut cursor = shift(1u64 << sq, dir);
        let mut line = 0;
        while cursor != 0 && cursor & board.opp != 0 {
            line |= cursor;
            cursor = shift(cursor, dir);
        }
        if cursor != 0 && cursor & board.me != 0 {
            flipped |= line;
        }
    }
    flipped
}

pub fn play(board: Board, sq: u8) -> Board {
    let bit = 1u64 << sq;
    debug_assert!(legal_moves(board.me, board.opp) & bit != 0, "illegal move");
    let flipped = flips(board, sq);
    debug_assert!(flipped != 0, "move flips no discs");
    Board {
        me: board.opp & !flipped,
        opp: board.me | bit | flipped,
    }
}

pub fn pass(board: Board) -> Board {
    Board {
        me: board.opp,
        opp: board.me,
    }
}

pub fn is_terminal(board: Board) -> bool {
    legal_moves(board.me, board.opp) == 0 && legal_moves(board.opp, board.me) == 0
}

pub fn disc_count(bits: u64) -> u32 {
    bits.count_ones()
}

pub fn empty_count(board: Board) -> u32 {
    64 - (board.me | board.opp).count_ones()
}

/// WTHOR score from the side to move; meaningful only on terminal boards.
pub fn final_score(board: Board) -> i32 {
    let me = disc_count(board.me) as i32;
    let opp = disc_count(board.opp) as i32;
    let empty = empty_count(board) as i32;
    if me > opp {
        me + empty - opp
    } else if me < opp {
        me - opp - empty
    } else {
        0
    }
}
