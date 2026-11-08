use crate::board::{Board, legal_moves, play, squares};
use crate::rng::Rng;
use crate::search::{SearchConfig, SearchLimits, search_tt};
use crate::tt::{MAX_ENTRIES, Table};

pub trait Agent {
    fn choose(&mut self, board: Board) -> Option<u8>;
    fn reset(&mut self) {}
}

/// Holds the initial state for `reset` and the live state.
pub struct RandomAgent(Rng, Rng);

impl Agent for RandomAgent {
    fn choose(&mut self, board: Board) -> Option<u8> {
        let moves = legal_moves(board.me, board.opp);
        (moves != 0).then(|| {
            let k = (self.1.next_u64() % moves.count_ones() as u64) as usize;
            squares(moves).nth(k).unwrap()
        })
    }

    fn reset(&mut self) {
        self.1 = self.0.clone();
    }
}

pub fn random_agent(seed: u64) -> RandomAgent {
    let rng = Rng::new(seed);
    RandomAgent(rng.clone(), rng)
}

pub struct GreedyAgent;

impl Agent for GreedyAgent {
    fn choose(&mut self, board: Board) -> Option<u8> {
        squares(legal_moves(board.me, board.opp))
            .max_by_key(|&s| (play(board, s).opp.count_ones(), std::cmp::Reverse(s)))
    }
}

pub fn greedy_agent() -> GreedyAgent {
    GreedyAgent
}

pub struct SearchAgent(SearchConfig, SearchLimits, Table);

impl Agent for SearchAgent {
    fn choose(&mut self, board: Board) -> Option<u8> {
        (legal_moves(board.me, board.opp) != 0)
            .then(|| search_tt(board, self.1, &self.0, &mut self.2).best)
    }

    fn reset(&mut self) {
        self.2.clear();
    }
}

pub fn search_agent(config: SearchConfig, limits: SearchLimits) -> SearchAgent {
    SearchAgent(config, limits, Table::new(MAX_ENTRIES))
}
