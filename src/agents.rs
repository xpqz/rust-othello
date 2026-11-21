use crate::board::{Board, legal_moves, play, squares};
use crate::rng::Rng;
use crate::search::{ClockPolicy, SearchConfig, SearchLimits, allot_with, search_tt};
use crate::tt::{MAX_ENTRIES, Table};

pub trait Agent {
    fn choose(&mut self, board: Board) -> Option<u8>;
    fn reset(&mut self) {}
    /// The remaining game clock before the next `choose`; ignored by default.
    fn clock(&mut self, _remaining_ms: u64) {}
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

/// Configuration, fixed limits, table, the clock policy of a clocked agent,
/// and the remaining clock once given.
pub struct SearchAgent(
    SearchConfig,
    SearchLimits,
    Table,
    Option<&'static ClockPolicy>,
    Option<u64>,
);

impl Agent for SearchAgent {
    fn choose(&mut self, board: Board) -> Option<u8> {
        let moves = legal_moves(board.me, board.opp);
        // A clocked, forced policy plays a lone move without searching.
        let forced = self.4.is_some() && self.3.is_some_and(ClockPolicy::forced);
        if moves.count_ones() == 1 && forced {
            return Some(moves.trailing_zeros() as u8);
        }
        (moves != 0).then(|| search_tt(board, self.limits(board), &self.0, &mut self.2).best)
    }

    fn reset(&mut self) {
        self.2.clear();
    }

    fn clock(&mut self, remaining_ms: u64) {
        if self.3.is_some() {
            self.4 = Some(remaining_ms);
        }
    }
}

impl SearchAgent {
    /// The limits the next `choose` on `board` would use.
    pub fn limits(&self, board: Board) -> SearchLimits {
        match (self.3, self.4) {
            (Some(policy), Some(remaining)) => {
                allot_with(board, remaining, self.0.endgame_threshold, policy)
            }
            _ => self.1,
        }
    }
}

pub fn search_agent(config: SearchConfig, limits: SearchLimits) -> SearchAgent {
    SearchAgent(config, limits, Table::new(MAX_ENTRIES), None, None)
}

/// A search agent that allocates from the game clock once `clock` is called,
/// and uses `limits` until then.
pub fn clock_agent(
    config: SearchConfig,
    limits: SearchLimits,
    policy: &'static ClockPolicy,
) -> SearchAgent {
    SearchAgent(config, limits, Table::new(MAX_ENTRIES), Some(policy), None)
}

/// The game clock for play (`docs/design/time.md`, gate #163): C = 51 s
/// per game under clock policy variant 3.
pub const PLAY_CLOCK_MS: u64 = 51_000;
pub const PLAY_POLICY: usize = 3;

/// The engine side of play: a clocked search under `PLAY_POLICY`.
pub fn play_agent() -> SearchAgent {
    let policy = &crate::search::CLOCK_POLICIES[PLAY_POLICY];
    clock_agent(
        SearchConfig::standard(),
        SearchLimits::new(64, 3000, 0),
        policy,
    )
}

/// One side's game clock: the time charged so far against a limit.
#[derive(Clone, Copy, Debug)]
pub struct GameClock {
    limit_ms: u64,
    used: std::time::Duration,
}

impl GameClock {
    pub fn new(limit_ms: u64) -> Self {
        Self {
            limit_ms,
            used: std::time::Duration::ZERO,
        }
    }

    /// Whole milliseconds left, rounded down; zero once spent.
    pub fn remaining_ms(&self) -> u64 {
        let left = std::time::Duration::from_millis(self.limit_ms).saturating_sub(self.used);
        left.as_millis() as u64
    }

    pub fn charge(&mut self, spent: std::time::Duration) {
        self.used = self.used.saturating_add(spent);
    }

    /// Gives back time charged for a move that was taken back.
    pub fn refund(&mut self, spent: std::time::Duration) {
        self.used = self.used.saturating_sub(spent);
    }
}
