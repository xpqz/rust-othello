pub const NOT_A: u64 = 0xfefe_fefe_fefe_fefe;
pub const NOT_H: u64 = 0x7f7f_7f7f_7f7f_7f7f;

const DIRS: [(i8, u64); 8] = [
    (1, NOT_A),
    (-1, NOT_H),
    (8, !0),
    (-8, !0),
    (9, NOT_A),
    (7, NOT_H),
    (-7, NOT_A),
    (-9, NOT_H),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    E,
    W,
    N,
    S,
    NE,
    NW,
    SE,
    SW,
}

impl Dir {
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

pub fn shift(bits: u64, dir: Dir) -> u64 {
    let (step, mask) = DIRS[dir as usize];
    if step > 0 {
        (bits << (step as u32)) & mask
    } else {
        (bits >> ((-step) as u32)) & mask
    }
}

pub fn legal_moves(me: u64, opp: u64) -> u64 {
    let empty = !(me | opp);
    let mut moves = 0;
    for dir in Dir::ALL {
        let mut run = shift(me, dir) & opp;
        let mut flips = 0;
        while run != 0 {
            flips |= run;
            run = shift(run, dir) & opp;
        }
        moves |= shift(flips, dir) & empty;
    }
    moves
}
