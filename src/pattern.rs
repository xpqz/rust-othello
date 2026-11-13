use std::sync::LazyLock;

use crate::board::{Board, empty_count};
use crate::score::EVAL_BOUND;

pub const INSTANCE_COUNT: usize = 46;
pub const TABLE_SLOTS: usize = 167_265;
pub const STAGES: usize = 12;

pub const FAMILIES: [&[u8]; 11] = [
    &[0, 1, 2, 3, 4, 5, 6, 7, 9, 14],
    &[0, 1, 2, 8, 9, 10, 16, 17, 18],
    &[0, 1, 2, 3, 4, 8, 9, 10, 11, 12],
    &[8, 9, 10, 11, 12, 13, 14, 15],
    &[16, 17, 18, 19, 20, 21, 22, 23],
    &[24, 25, 26, 27, 28, 29, 30, 31],
    &[0, 9, 18, 27, 36, 45, 54, 63],
    &[1, 10, 19, 28, 37, 46, 55],
    &[2, 11, 20, 29, 38, 47],
    &[3, 12, 21, 30, 39],
    &[4, 13, 22, 31],
];

pub struct Instance {
    pub squares: Vec<u8>,
    pub family: usize,
    pub offset: u32,
}

static INSTANCES: LazyLock<Vec<Instance>> = LazyLock::new(build);

pub fn instances() -> &'static [Instance] {
    INSTANCES.as_slice()
}

/// `ROWS[row * 256 + byte][instance]`: what the mover's discs `byte` on `row`
/// add to that instance's base-3 index. The opponent's discs add twice.
static ROWS: LazyLock<Vec<[u16; INSTANCE_COUNT]>> = LazyLock::new(|| {
    let mut rows = vec![[0u16; INSTANCE_COUNT]; 8 * 256];
    for (slot, instance) in instances().iter().enumerate() {
        let mut power = 1;
        for &square in &instance.squares {
            let (row, file) = (usize::from(square / 8), square % 8);
            for byte in (0..256).filter(|byte| byte >> file & 1 != 0) {
                rows[row * 256 + byte][slot] += power;
            }
            power *= 3;
        }
    }
    rows
});

pub fn features(board: Board) -> [u32; INSTANCE_COUNT] {
    let mut sums = [0u16; INSTANCE_COUNT];
    for row in 0..8 {
        let me = &ROWS[row * 256 + (board.me >> (row * 8) & 0xff) as usize];
        let opp = &ROWS[row * 256 + (board.opp >> (row * 8) & 0xff) as usize];
        for ((sum, &me), &opp) in sums.iter_mut().zip(me).zip(opp) {
            *sum += me + 2 * opp;
        }
    }
    let mut out = [0; INSTANCE_COUNT];
    for ((out, sum), offset) in out.iter_mut().zip(sums).zip(*OFFSETS) {
        *out = offset + u32::from(sum);
    }
    out
}

/// Each instance's table offset, in instance order.
static OFFSETS: LazyLock<[u32; INSTANCE_COUNT]> =
    LazyLock::new(|| std::array::from_fn(|slot| instances()[slot].offset));

static WEIGHTS: &[u8; 4_014_360] = include_bytes!("../assets/eval.bin");

/// The file's little-endian weights decoded once: `TABLE[stage * TABLE_SLOTS + slot]`.
static TABLE: LazyLock<Vec<i16>> = LazyLock::new(|| {
    let (pairs, _) = WEIGHTS.as_chunks::<2>();
    pairs.iter().map(|&pair| i16::from_le_bytes(pair)).collect()
});

pub fn evaluate(board: Board) -> i32 {
    let stage = stage(empty_count(board));
    let mut value = 0;
    let table = &TABLE[stage * TABLE_SLOTS..][..TABLE_SLOTS];
    for feature in features(board) {
        value += i32::from(table[feature as usize]);
    }
    value.clamp(-EVAL_BOUND, EVAL_BOUND)
}

pub fn stage(empties: u32) -> usize {
    (60u32.saturating_sub(empties) / 5).min(11) as usize
}

fn build() -> Vec<Instance> {
    let mut instances = Vec::with_capacity(INSTANCE_COUNT);
    let mut offset = 0;
    for (family, base) in FAMILIES.iter().enumerate() {
        let mut seen = Vec::new();
        for kind in 0..8 {
            let squares: Vec<u8> = base.iter().map(|&square| transform(square, kind)).collect();
            let mask = squares
                .iter()
                .fold(0u64, |mask, &square| mask | (1u64 << square));
            if !seen.contains(&mask) {
                seen.push(mask);
                instances.push(Instance {
                    squares,
                    family,
                    offset,
                });
            }
        }
        offset += 3u32.pow(base.len() as u32);
    }
    instances
}

fn transform(square: u8, kind: usize) -> u8 {
    let file = square % 8;
    let rank = square / 8;
    let (file, rank) = match kind {
        0 => (file, rank),
        1 => (7 - rank, file),
        2 => (7 - file, 7 - rank),
        3 => (rank, 7 - file),
        4 => (7 - file, rank),
        5 => (file, 7 - rank),
        6 => (rank, file),
        _ => (7 - rank, 7 - file),
    };
    rank * 8 + file
}
