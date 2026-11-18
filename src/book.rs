//! Opening book lookup (`docs/design/book.md` § Engine).

use crate::board::Board;
use crate::search::{SearchConfig, SearchLimits, SearchResult, search_tt};
use crate::tt::Table;

/// Sorted 11-byte little-endian records: `tt::zobrist` of the canonical
/// board, the move as a canonical square, and the value as i16.
pub static BOOK: &[u8] = include_bytes!("../assets/book.bin");
pub const RECORD: usize = 11;

/// The stored move, mapped back to `board`'s orientation, and its value.
pub fn lookup(_asset: &[u8], _board: Board) -> Option<(u8, i32)> {
    None
}

pub fn book_move(board: Board) -> Option<(u8, i32)> {
    lookup(BOOK, board)
}

/// A book hit when `config.book` is set; otherwise `search_tt`.
pub fn play(
    board: Board,
    limits: SearchLimits,
    config: &SearchConfig,
    table: &mut Table,
) -> SearchResult {
    search_tt(board, limits, config, table)
}
