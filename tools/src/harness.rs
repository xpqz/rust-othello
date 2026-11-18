use std::time::{Duration, Instant};

use othello::agents::Agent;
use othello::board::{Board, disc_count, final_score, legal_moves, pass, play};
use othello::score::terminal_score;

pub type Factory = dyn Fn() -> Box<dyn Agent>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameResult {
    /// Terminal centidiscs from black's perspective.
    pub score: i32,
    /// Placements plus passes.
    pub plies: u32,
    pub passes: u32,
}

pub fn play_game(
    opening: &[u8],
    black: &mut dyn Agent,
    white: &mut dyn Agent,
) -> Result<GameResult, String> {
    let real = &mut |start: Instant| start.elapsed();
    Ok(play_clocked(opening, black, white, [None, None], real)?.result)
}

pub fn play_from(
    board: Board,
    black_to_move: bool,
    black: &mut dyn Agent,
    white: &mut dyn Agent,
) -> Result<GameResult, String> {
    let real = &mut |start: Instant| start.elapsed();
    Ok(clocked_from(board, black_to_move, [black, white], [None, None], real)?.result)
}

/// The game from `board` under `clocks`, indexed black then white.
fn clocked_from(
    mut board: Board,
    mut black_to_move: bool,
    agents: [&mut dyn Agent; 2],
    clocks: [Option<Duration>; 2],
    elapsed: &mut dyn FnMut(Instant) -> Duration,
) -> Result<Clocked, String> {
    let [black, white] = agents;
    let (mut plies, mut passes, mut used) = (0, 0, [Duration::ZERO; 2]);
    let (value, flag) = loop {
        let moves = legal_moves(board.me, board.opp);
        if moves == 0 {
            if legal_moves(board.opp, board.me) == 0 {
                break (terminal_score(final_score(board)), None);
            }
            board = pass(board);
            black_to_move = !black_to_move;
            plies += 1;
            passes += 1;
            continue;
        }
        let side = usize::from(!black_to_move);
        let agent: &mut dyn Agent = if black_to_move {
            &mut *black
        } else {
            &mut *white
        };
        if let Some(limit) = clocks[side] {
            agent.clock(limit.saturating_sub(used[side]).as_millis() as u64);
        }
        let start = Instant::now();
        let square = agent.choose(board);
        used[side] += elapsed(start);
        if clocks[side].is_some_and(|limit| used[side] > limit) {
            // The flagging move is not played; the score is the stopped lead.
            let lead = disc_count(board.me) as i32 - disc_count(board.opp) as i32;
            break (terminal_score(lead), Some(side));
        }
        let square = square.ok_or("agent returned None with legal moves")?;
        if !contains(moves, square) {
            return Err(format!("illegal move: {square}"));
        }
        board = play(board, square);
        black_to_move = !black_to_move;
        plies += 1;
    };
    Ok(Clocked {
        result: GameResult {
            score: if black_to_move { value } else { -value },
            plies,
            passes,
        },
        used,
        flag,
    })
}

fn contains(moves: u64, square: u8) -> bool {
    square < 64 && moves & (1u64 << square) != 0
}

pub fn points(score: i32) -> f64 {
    match score.signum() {
        1 => 1.0,
        0 => 0.5,
        _ => 0.0,
    }
}

pub fn pair_score(opening: &[u8], first: &Factory, second: &Factory) -> Result<f64, String> {
    let mut first_black = first();
    let mut second_white = second();
    let first_points = points(play_game(opening, &mut *first_black, &mut *second_white)?.score);
    let mut second_black = second();
    let mut first_white = first();
    let second_points =
        1.0 - points(play_game(opening, &mut *second_black, &mut *first_white)?.score);
    Ok((first_points + second_points) / 2.0)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameLog {
    pub run: u64,
    pub pair: usize,
    pub game: u64,
    pub agent_seeds: (u64, u64),
    pub key: (u64, u64),
    pub opening: String,
    pub black: String,
    pub white: String,
    pub score: i32,
}

impl GameLog {
    pub fn line(&self) -> String {
        format!(
            "run {} pair {} game {} seeds ({}, {}) key ({}, {}) opening {} black {} white {} score {}",
            self.run,
            self.pair,
            self.game,
            self.agent_seeds.0,
            self.agent_seeds.1,
            self.key.0,
            self.key.1,
            self.opening,
            self.black,
            self.white,
            self.score
        )
    }
}

/// A game under per-side clocks (`docs/design/time.md` § Clock contract).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clocked {
    pub result: GameResult,
    /// Time charged to black and white: every `choose` call, in full.
    pub used: [Duration; 2],
    /// The side (0 black, 1 white) whose clock ran out; its last move is
    /// not played.
    pub flag: Option<usize>,
}

/// Black's points: a flag loses whatever the discs.
pub fn clocked_points(game: &Clocked) -> f64 {
    match game.flag {
        Some(side) => side as f64,
        None => points(game.result.score),
    }
}

/// Plays a game, charging each `choose` call `elapsed(start of call)`.
/// A side with a clock receives `Agent::clock` before each move and flags
/// once its charged time exceeds the clock; its last move is not played.
pub fn play_clocked(
    opening: &[u8],
    black: &mut dyn Agent,
    white: &mut dyn Agent,
    clocks: [Option<Duration>; 2],
    elapsed: &mut dyn FnMut(Instant) -> Duration,
) -> Result<Clocked, String> {
    let mut board = Board::START;
    for &square in opening {
        if !contains(legal_moves(board.me, board.opp), square) {
            return Err("illegal opening move".into());
        }
        board = play(board, square);
    }
    let black_to_move = opening.len().is_multiple_of(2);
    let mut game = clocked_from(board, black_to_move, [black, white], clocks, elapsed)?;
    game.result.plies += opening.len() as u32;
    Ok(game)
}
