#![allow(dead_code)]

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
