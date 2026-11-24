use othello::board::Board;
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
pub fn quick_move(_level: Level, _board: Board, _rng: &mut Rng) -> Option<u8> {
    None
}
