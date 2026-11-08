use crate::board::Board;

pub const NO_MOVE: u8 = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Domain {
    #[default]
    Heuristic,
    Endgame,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bound {
    #[default]
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub key: u64,
    pub value: i32,
    pub depth: u8,
    pub flag: Bound,
    pub domain: Domain,
    pub mv: u8,
}

impl Entry {
    pub fn heuristic(key: u64, value: i32, depth: u8, flag: Bound, mv: u8) -> Self {
        Self {
            key,
            value,
            depth,
            flag,
            domain: Domain::Heuristic,
            mv,
        }
    }

    fn window(&self, alpha: i32, beta: i32, domain: Domain) -> Option<i32> {
        if self.domain != domain {
            return None;
        }
        match self.flag {
            Bound::Exact => Some(self.value),
            Bound::Lower if self.value >= beta => Some(self.value),
            Bound::Upper if self.value <= alpha => Some(self.value),
            _ => None,
        }
    }

    pub fn cutoff(&self, depth: u32, alpha: i32, beta: i32) -> Option<i32> {
        (self.depth as u32 == depth)
            .then(|| self.window(alpha, beta, Domain::Heuristic))
            .flatten()
    }

    pub fn endgame_cutoff(&self, alpha: i32, beta: i32) -> Option<i32> {
        self.window(alpha, beta, Domain::Endgame)
    }

    pub fn move_hint(&self, depth: u32) -> Option<u8> {
        (self.domain == Domain::Heuristic && self.depth as u32 >= depth && self.mv != NO_MOVE)
            .then_some(self.mv)
    }
}

pub struct Table(Vec<Entry>);

impl Table {
    pub fn new(entries: usize) -> Self {
        Self(vec![Entry::default(); entries])
    }

    pub fn clear(&mut self) {
        self.0.fill(Entry::default());
    }

    pub fn probe(&self, key: u64) -> Option<&Entry> {
        if key == 0 || self.0.is_empty() {
            return None;
        }
        let entry = &self.0[key as usize % self.0.len()];
        (entry.key == key).then_some(entry)
    }

    pub fn store(&mut self, entry: Entry) {
        if entry.key == 0 || self.0.is_empty() {
            return;
        }
        let index = entry.key as usize % self.0.len();
        self.0[index] = entry;
    }
}

const fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

const fn tables() -> [[u64; 64]; 2] {
    let mut tables = [[0u64; 64]; 2];
    let mut index = 0;
    while index < 64 {
        tables[0][index] = splitmix64(index as u64);
        tables[1][index] = splitmix64(128 + index as u64);
        index += 1;
    }
    tables
}

const ZOBRIST: [[u64; 64]; 2] = tables();

fn mix(mut bits: u64, colour: usize) -> u64 {
    let mut key = 0;
    while bits != 0 {
        key ^= ZOBRIST[colour][bits.trailing_zeros() as usize];
        bits &= bits - 1;
    }
    key
}

pub fn zobrist(board: Board) -> u64 {
    mix(board.me, 0) ^ mix(board.opp, 1)
}
