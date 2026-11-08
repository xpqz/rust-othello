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
