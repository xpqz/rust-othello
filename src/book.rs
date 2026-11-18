//! Opening book lookup (`docs/design/book.md` § Engine).

use crate::board::{Board, legal_moves};
use crate::search::{SearchConfig, SearchLimits, SearchResult, SearchStatus, search_tt};
use crate::tt::{Table, zobrist};

/// Sorted 11-byte little-endian records: `tt::zobrist` of the canonical
/// board, the move as a canonical square, and the value as i16.
pub static BOOK: &[u8] = include_bytes!("../assets/book.bin");
pub const RECORD: usize = 11;

/// Swaps file and rank: (f, r) -> (r, f).
fn transpose(mut x: u64) -> u64 {
    for (shift, mask) in [
        (28, 0x0f0f_0f0f_0000_0000u64),
        (14, 0x3333_0000_3333_0000),
        (7, 0x5500_5500_5500_5500),
    ] {
        let t = mask & (x ^ (x << shift));
        x ^= t ^ (t >> shift);
    }
    x
}

/// The image of `bits` under symmetry `kind`, numbered as in `pattern.rs`.
fn image(bits: u64, kind: u8) -> u64 {
    let mirror = |x: u64| x.reverse_bits().swap_bytes();
    match kind {
        0 => bits,
        1 => mirror(transpose(bits)),
        2 => bits.reverse_bits(),
        3 => transpose(bits).swap_bytes(),
        4 => mirror(bits),
        5 => bits.swap_bytes(),
        6 => transpose(bits),
        _ => transpose(bits).reverse_bits(),
    }
}

/// The stored move, mapped back to `board`'s orientation, and its value.
/// The canonical board is the least `(me, opp)` over the eight images, as
/// in the builder. A board without a legal move is never answered.
pub fn lookup(asset: &[u8], board: Board) -> Option<(u8, i32)> {
    let moves = legal_moves(board.me, board.opp);
    let (records, rest) = asset.as_chunks::<RECORD>();
    if moves == 0 || !rest.is_empty() {
        return None;
    }
    let (kind, key) = (0..8)
        .map(|k| (k, (image(board.me, k), image(board.opp, k))))
        .min_by_key(|&(_, key)| key)?;
    let target = zobrist(Board {
        me: key.0,
        opp: key.1,
    });
    let index = records
        .binary_search_by_key(&target, |r| u64::from_le_bytes(r[..8].try_into().unwrap()))
        .ok()?;
    let record = records[index];
    if record[8] >= 64 {
        return None;
    }
    let inverse = [0, 3, 2, 1, 4, 5, 6, 7][kind as usize];
    let square = image(1 << record[8], inverse).trailing_zeros() as u8;
    let value = i16::from_le_bytes([record[9], record[10]]).into();
    (moves & (1 << square) != 0).then_some((square, value))
}

pub fn book_move(board: Board) -> Option<(u8, i32)> {
    lookup(BOOK, board)
}

/// The book move when `config.book` is set and the book answers, as a
/// complete depth-0 result with no nodes; otherwise `search_tt`.
pub fn play(
    board: Board,
    limits: SearchLimits,
    config: &SearchConfig,
    table: &mut Table,
) -> SearchResult {
    match config.book.then(|| book_move(board)).flatten() {
        Some((best, score)) => SearchResult {
            best,
            score,
            depth: 0,
            nodes: 0,
            status: SearchStatus::Complete,
            solved: false,
        },
        None => search_tt(board, limits, config, table),
    }
}
