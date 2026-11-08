#![allow(dead_code)]

use othello::board::{Board, legal_moves, pass, play};
use othello::rng::Rng;

const DIRECTIONS: [(i8, i8); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

pub fn format_bits(me: u64, opp: u64) -> String {
    let mut out = String::new();
    for rank in (0..8).rev() {
        for file in 0..8 {
            let bit = 1u64 << (rank * 8 + file);
            out.push(if me & bit != 0 {
                'X'
            } else if opp & bit != 0 {
                'O'
            } else {
                '.'
            });
        }
        out.push('\n');
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sym {
    Identity,
    Rot90,
    Rot180,
    Rot270,
    FlipH,
    FlipV,
    FlipDiag,
    FlipAnti,
}

impl Sym {
    pub const ALL: [Sym; 8] = [
        Sym::Identity,
        Sym::Rot90,
        Sym::Rot180,
        Sym::Rot270,
        Sym::FlipH,
        Sym::FlipV,
        Sym::FlipDiag,
        Sym::FlipAnti,
    ];
}

pub fn transform(bits: u64, sym: Sym) -> u64 {
    let mut out = 0;
    let mut rest = bits;
    while rest != 0 {
        let square = rest.trailing_zeros() as usize;
        rest &= rest - 1;
        let file = square % 8;
        let rank = square / 8;
        let (file, rank) = match sym {
            Sym::Identity => (file, rank),
            Sym::Rot90 => (7 - rank, file),
            Sym::Rot180 => (7 - file, 7 - rank),
            Sym::Rot270 => (rank, 7 - file),
            Sym::FlipH => (7 - file, rank),
            Sym::FlipV => (file, 7 - rank),
            Sym::FlipDiag => (rank, file),
            Sym::FlipAnti => (7 - rank, 7 - file),
        };
        out |= 1u64 << (rank * 8 + file);
    }
    out
}

pub struct Naive {
    pub cells: [[i8; 8]; 8],
}

impl Naive {
    pub fn start() -> Self {
        let mut cells = [[0; 8]; 8];
        cells[4][3] = 1;
        cells[3][4] = 1;
        cells[3][3] = -1;
        cells[4][4] = -1;
        Self { cells }
    }

    pub fn from_bits(me: u64, opp: u64) -> Self {
        let mut cells = [[0; 8]; 8];
        for (rank, row) in cells.iter_mut().enumerate() {
            for (file, cell) in row.iter_mut().enumerate() {
                let bit = 1u64 << (rank * 8 + file);
                *cell = if me & bit != 0 {
                    1
                } else if opp & bit != 0 {
                    -1
                } else {
                    0
                };
            }
        }
        Self { cells }
    }

    fn line(&self, file: i8, rank: i8, df: i8, dr: i8) -> Vec<(usize, usize)> {
        let mut run = Vec::new();
        let (mut file, mut rank) = (file + df, rank + dr);
        while (0..8).contains(&file)
            && (0..8).contains(&rank)
            && self.cells[rank as usize][file as usize] == -1
        {
            run.push((file as usize, rank as usize));
            file += df;
            rank += dr;
        }
        if !run.is_empty()
            && (0..8).contains(&file)
            && (0..8).contains(&rank)
            && self.cells[rank as usize][file as usize] == 1
        {
            run
        } else {
            Vec::new()
        }
    }

    pub fn flips(&self, sq: u8) -> u64 {
        let file = (sq % 8) as i8;
        let rank = (sq / 8) as i8;
        let mut mask = 0;
        for (df, dr) in DIRECTIONS {
            for (file, rank) in self.line(file, rank, df, dr) {
                mask |= 1u64 << (rank * 8 + file);
            }
        }
        mask
    }

    pub fn legal_moves(&self) -> u64 {
        let mut moves = 0;
        for rank in 0..8 {
            for file in 0..8 {
                let square = (rank * 8 + file) as u8;
                if self.cells[rank][file] == 0 && self.flips(square) != 0 {
                    moves |= 1u64 << square;
                }
            }
        }
        moves
    }

    pub fn play(&self, sq: u8) -> Self {
        let mut next = Self { cells: self.cells };
        let file = (sq % 8) as i8;
        let rank = (sq / 8) as i8;
        for (df, dr) in DIRECTIONS {
            for (file, rank) in self.line(file, rank, df, dr) {
                next.cells[rank][file] = 1;
            }
        }
        next.cells[rank as usize][file as usize] = 1;
        next.negate()
    }

    pub fn pass(&self) -> Self {
        Self { cells: self.cells }.negate()
    }

    fn negate(mut self) -> Self {
        for row in &mut self.cells {
            for cell in row {
                *cell = -*cell;
            }
        }
        self
    }

    pub fn bits(&self) -> (u64, u64) {
        let mut me = 0;
        let mut opp = 0;
        for rank in 0..8 {
            for file in 0..8 {
                let bit = 1u64 << (rank * 8 + file);
                match self.cells[rank][file] {
                    1 => me |= bit,
                    -1 => opp |= bit,
                    _ => {}
                }
            }
        }
        (me, opp)
    }
}

pub fn nth_bit(mut bits: u64, mut k: u32) -> u8 {
    while k > 0 {
        bits &= bits - 1;
        k -= 1;
    }
    bits.trailing_zeros() as u8
}

pub fn playout(rng: &mut Rng, mut visit: impl FnMut(Board)) {
    let mut board = Board::START;
    for _ in 0..60 {
        visit(board);
        let moves = legal_moves(board.me, board.opp);
        if moves != 0 {
            let k = (rng.next_u64() % moves.count_ones() as u64) as u32;
            board = play(board, nth_bit(moves, k));
        } else if legal_moves(board.opp, board.me) == 0 {
            break;
        } else {
            board = pass(board);
        }
    }
}
