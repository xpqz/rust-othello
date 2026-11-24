use std::cmp::Reverse;

use othello::agents::{Agent, greedy_agent};
use othello::board::{Board, legal_moves, play, squares};
use othello::pattern::evaluate;
use othello::rng::Rng;

/// The engine's playing strength in the GUI, weakest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Random,
    Greedy,
    Static,
    Search,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Random, Level::Greedy, Level::Static, Level::Search];

    pub fn label(self) -> &'static str {
        match self {
            Level::Random => "0 · Random move",
            Level::Greedy => "1 · Greedy",
            Level::Static => "2 · Best evaluation",
            Level::Search => "3 · Full search",
        }
    }
}

/// The move a no-search level plays: `None` at `Search` or without a legal move.
pub fn quick_move(level: Level, board: Board, rng: &mut Rng) -> Option<u8> {
    let moves = legal_moves(board.me, board.opp);
    match level {
        Level::Random => (moves != 0).then(|| {
            let k = rng.next_u64() % u64::from(moves.count_ones());
            squares(moves).nth(k as usize).unwrap()
        }),
        Level::Greedy => greedy_agent().choose(board),
        Level::Static => squares(moves).max_by_key(|&s| (-evaluate(play(board, s)), Reverse(s))),
        Level::Search => None,
    }
}
