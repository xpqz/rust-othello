use crate::board::{Board, legal_moves, play, squares};
use crate::rng::Rng;
use crate::search::{SearchConfig, SearchLimits, search};

pub trait Agent {
    fn choose(&mut self, board: Board) -> Option<u8>;
    fn reset(&mut self) {}
}

pub struct RandomAgent {
    seed: u64,
    rng: Rng,
}

impl Agent for RandomAgent {
    fn choose(&mut self, board: Board) -> Option<u8> {
        let moves = legal_moves(board.me, board.opp);
        let k = (self.rng.next_u64() % moves.count_ones().max(1) as u64) as usize;
        (moves != 0).then(|| squares(moves).nth(k).unwrap())
    }

    fn reset(&mut self) {
        self.rng = Rng::new(self.seed);
    }
}

pub fn random_agent(seed: u64) -> RandomAgent {
    RandomAgent {
        seed,
        rng: Rng::new(seed),
    }
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

pub struct SearchAgent {
    config: SearchConfig,
    limits: SearchLimits,
}

impl Agent for SearchAgent {
    fn choose(&mut self, board: Board) -> Option<u8> {
        (legal_moves(board.me, board.opp) != 0)
            .then(|| search(board, self.limits.max_depth, &self.config).best)
    }
}

pub fn search_agent(config: SearchConfig, limits: SearchLimits) -> SearchAgent {
    SearchAgent { config, limits }
}
