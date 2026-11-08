mod common;

use common::playout;
use othello::agents::{Agent, greedy_agent, random_agent, search_agent};
use othello::board::{Board, is_terminal, legal_moves};
use othello::eval::evaluate;
use othello::rng::Rng;
use othello::search::{SearchConfig, SearchLimits, search};

fn at(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

fn sq(file: usize, rank: usize) -> u8 {
    (rank * 8 + file) as u8
}

fn config() -> SearchConfig {
    SearchConfig {
        eval: evaluate,
        endgame_threshold: 12,
    }
}

fn limits() -> SearchLimits {
    SearchLimits::depth(2)
}

#[test]
fn random_legal() {
    let mut rng = Rng::new(0xA9E7);
    let mut agent = random_agent(7);
    for _ in 0..50 {
        playout(&mut rng, |board| {
            let moves = legal_moves(board.me, board.opp);
            match agent.choose(board) {
                Some(square) => assert_ne!(moves & (1 << square), 0),
                None => assert_eq!(moves, 0),
            }
        });
    }
}

#[test]
fn greedy_hand_checked() {
    let mut agent = greedy_agent();
    assert_eq!(agent.choose(Board::START), Some(sq(3, 2)));
    let board = Board {
        me: at(2, 0) | at(6, 0),
        opp: at(1, 0) | at(3, 0) | at(5, 0),
    };
    assert_eq!(legal_moves(board.me, board.opp), at(0, 0) | at(4, 0));
    assert_eq!(agent.choose(board), Some(sq(4, 0)));
}

#[test]
fn search_legal() {
    let cfg = config();
    let mut agent = search_agent(cfg, limits());
    for board in [
        Board::START,
        Board {
            me: at(2, 0) | at(2, 1),
            opp: at(1, 0) | at(1, 1),
        },
    ] {
        assert_eq!(agent.choose(board), Some(search(board, 2, &cfg).best));
    }
    let mut rng = Rng::new(0x5EA3);
    for _ in 0..20 {
        playout(&mut rng, |board| {
            let moves = legal_moves(board.me, board.opp);
            match agent.choose(board) {
                Some(square) => assert_ne!(moves & (1 << square), 0),
                None => assert_eq!(moves, 0),
            }
        });
    }
}

#[test]
fn agent_roots() {
    let mut search = search_agent(config(), limits());
    let pass_root = Board {
        me: at(1, 1),
        opp: at(0, 0) | at(2, 0),
    };
    assert_eq!(legal_moves(pass_root.me, pass_root.opp), 0);
    assert_eq!(search.choose(pass_root), None);

    let terminal = Board {
        me: at(0, 0),
        opp: at(7, 7),
    };
    assert!(is_terminal(terminal));
    assert_eq!(search.choose(terminal), None);

    let mut greedy = greedy_agent();
    assert_eq!(greedy.choose(pass_root), None);
    let mut random = random_agent(0);
    assert_eq!(random.choose(terminal), None);
}

#[test]
fn reset_replays() {
    let board = Board::START;
    let mut agent = random_agent(42);
    let first: Vec<_> = (0..8).map(|_| agent.choose(board)).collect();
    agent.reset();
    let again: Vec<_> = (0..8).map(|_| agent.choose(board)).collect();
    assert_eq!(first, again);
}

#[test]
fn game_finishes() {
    let mut first = random_agent(1);
    let mut second = random_agent(2);
    assert!(is_terminal(common::agent_game(
        Board::START,
        &mut first,
        &mut second
    )));

    let pass_start = Board {
        me: at(1, 1),
        opp: at(0, 0) | at(2, 0),
    };
    let mut first = random_agent(3);
    let mut second = random_agent(4);
    assert!(is_terminal(common::agent_game(
        pass_start,
        &mut first,
        &mut second
    )));
}
