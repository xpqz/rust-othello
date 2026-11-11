use std::sync::LazyLock;

use crate::board::Board;

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

pub fn features(board: Board) -> [u32; INSTANCE_COUNT] {
    let mut out = [0; INSTANCE_COUNT];
    for (slot, instance) in out.iter_mut().zip(instances()) {
        let mut index = 0;
        let mut power = 1;
        for &square in &instance.squares {
            let bit = 1u64 << square;
            let digit = if board.me & bit != 0 {
                1
            } else if board.opp & bit != 0 {
                2
            } else {
                0
            };
            index += digit * power;
            power *= 3;
        }
        *slot = instance.offset + index;
    }
    out
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
