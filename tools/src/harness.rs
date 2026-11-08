use othello::agents::Agent;
use othello::board::{Board, final_score, legal_moves, pass, play};
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
    let mut board = Board::START;
    let mut black_to_move = true;
    for &square in opening {
        if !contains(legal_moves(board.me, board.opp), square) {
            return Err("illegal opening move".into());
        }
        board = play(board, square);
        black_to_move = !black_to_move;
    }
    let mut result = play_from(board, black_to_move, black, white)?;
    result.plies += opening.len() as u32;
    Ok(result)
}

pub fn play_from(
    board: Board,
    mut black_to_move: bool,
    black: &mut dyn Agent,
    white: &mut dyn Agent,
) -> Result<GameResult, String> {
    let mut board = board;
    let (mut plies, mut passes) = (0, 0);
    loop {
        let moves = legal_moves(board.me, board.opp);
        if moves == 0 {
            if legal_moves(board.opp, board.me) == 0 {
                let value = terminal_score(final_score(board));
                return Ok(GameResult {
                    score: if black_to_move { value } else { -value },
                    plies,
                    passes,
                });
            }
            board = pass(board);
            black_to_move = !black_to_move;
            plies += 1;
            passes += 1;
            continue;
        }
        let agent: &mut dyn Agent = if black_to_move {
            &mut *black
        } else {
            &mut *white
        };
        let square = agent
            .choose(board)
            .ok_or("agent returned None with legal moves")?;
        if !contains(moves, square) {
            return Err(format!("illegal move: {square}"));
        }
        board = play(board, square);
        black_to_move = !black_to_move;
        plies += 1;
    }
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
