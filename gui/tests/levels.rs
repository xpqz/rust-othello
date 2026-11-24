use std::collections::BTreeSet;

use othello::agents::{Agent, greedy_agent, random_agent};
use othello::board::{Board, is_terminal, legal_moves, pass, play, squares};
use othello::pattern::evaluate;
use othello::rng::Rng;
use othello_gui::levels::{Level, quick_move};

/// Positions with a legal move along a seeded random game.
fn playout(seed: u64) -> Vec<Board> {
    let mut agent = random_agent(seed);
    let mut board = Board::START;
    let mut positions = Vec::new();
    while !is_terminal(board) {
        match agent.choose(board) {
            Some(square) => {
                positions.push(board);
                board = play(board, square);
            }
            None => board = pass(board),
        }
    }
    positions
}

fn legal(board: Board, square: u8) -> bool {
    legal_moves(board.me, board.opp) & (1u64 << square) != 0
}

/// Level 0 plays only legal moves, reaches every opening move, and
/// repeats its choices for a repeated seed.
#[test]
fn random_legal_varied() {
    let draws = |seed| {
        let mut rng = Rng::new(seed);
        (0..200)
            .map(|_| quick_move(Level::Random, Board::START, &mut rng).unwrap())
            .collect::<Vec<_>>()
    };
    let picks = draws(7);
    assert!(picks.iter().all(|&s| legal(Board::START, s)));
    let seen: BTreeSet<u8> = picks.iter().copied().collect();
    let all: BTreeSet<u8> = squares(legal_moves(Board::START.me, Board::START.opp)).collect();
    assert_eq!(seen, all);
    assert_eq!(picks, draws(7));
}

/// Level 1 plays the engine's greedy move.
#[test]
fn greedy_matches_agent() {
    let mut rng = Rng::new(1);
    for board in playout(3) {
        assert_eq!(
            quick_move(Level::Greedy, board, &mut rng),
            greedy_agent().choose(board)
        );
    }
}

/// Level 2 plays a legal move whose static evaluation for the mover is
/// the best after one ply, and differs from greedy somewhere.
#[test]
fn static_best_evaluation() {
    let mut rng = Rng::new(1);
    let mut differs = false;
    for board in playout(5) {
        let pick = quick_move(Level::Static, board, &mut rng).expect("a legal move");
        assert!(legal(board, pick));
        let value = |s| -evaluate(play(board, s));
        let best = squares(legal_moves(board.me, board.opp)).map(value).max();
        assert_eq!(Some(value(pick)), best, "{board:?}");
        differs |= Some(pick) != greedy_agent().choose(board);
    }
    assert!(differs, "level 2 must not reduce to greedy");
}

/// Level 3 is left to search; no level moves without a legal move.
#[test]
fn search_and_stuck_none() {
    let mut rng = Rng::new(1);
    assert_eq!(quick_move(Level::Search, Board::START, &mut rng), None);
    let stuck = Board { me: 1, opp: 0 };
    for level in Level::ALL {
        assert_eq!(quick_move(level, stuck, &mut rng), None, "{level:?}");
    }
}
