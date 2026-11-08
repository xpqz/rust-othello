mod common;

use common::playout;
use othello::board::{Board, final_score, is_terminal, legal_moves, pass, play};
use othello::eval::evaluate;
use othello::rng::Rng;
use othello::score::{INF, terminal_score};
use othello::search::{Ctx, SearchConfig, SearchStatus, negamax, search};

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

fn minimax(board: Board, depth: u32, config: &SearchConfig) -> i32 {
    let moves = legal_moves(board.me, board.opp);
    if moves == 0 {
        if legal_moves(board.opp, board.me) == 0 {
            return terminal_score(final_score(board));
        }
        return -minimax(pass(board), depth, config);
    }
    if depth == 0 {
        return (config.eval)(board);
    }
    let mut best = -INF;
    let mut rest = moves;
    while rest != 0 {
        let square = rest.trailing_zeros() as u8;
        rest &= rest - 1;
        best = best.max(-minimax(play(board, square), depth - 1, config));
    }
    best
}

#[test]
fn depth_zero_terminal() {
    let config = config();
    let board = Board {
        me: at(0, 0) | at(1, 0),
        opp: at(7, 7),
    };
    let mut ctx = Ctx::new(&config);
    assert_eq!(negamax(board, 0, -INF, INF, &mut ctx), Some(6200));
    assert_ne!(evaluate(board), 6200);
}

#[test]
fn horizon_terminal() {
    let config = config();
    let board = Board {
        me: !(at(0, 0) | at(1, 0) | at(7, 7)),
        opp: at(1, 0) | at(7, 7),
    };
    assert_eq!(legal_moves(board.me, board.opp), at(0, 0));
    let child = play(board, sq(0, 0));
    assert!(is_terminal(child));
    assert_eq!(evaluate(child), -6112);
    assert_eq!(terminal_score(final_score(child)), -6200);
    let mut ctx = Ctx::new(&config);
    assert_eq!(negamax(board, 1, -INF, INF, &mut ctx), Some(6200));
}

#[test]
fn pass_in_tree() {
    let config = config();
    let board = Board {
        me: at(1, 0),
        opp: at(0, 0) | at(2, 0),
    };
    assert_eq!(legal_moves(board.me, board.opp), 0);
    assert!(!is_terminal(board));
    assert_ne!(legal_moves(board.opp, board.me), 0);
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        negamax(board, 2, -INF, INF, &mut ctx),
        Some(minimax(board, 2, &config))
    );
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        negamax(board, 2, -INF, INF, &mut ctx),
        Some(-negamax(pass(board), 2, -INF, INF, &mut Ctx::new(&config)).unwrap())
    );
}

#[test]
fn fail_soft() {
    let config = config();
    let mut ctx = Ctx::new(&config);
    let value = negamax(Board::START, 1, 5000, INF, &mut ctx).unwrap();
    assert_eq!(value, 297);
    assert!(value < 5000);
}

#[test]
fn minimax_equivalence() {
    let config = config();
    let mut rng = Rng::new(0x5EA2);
    let mut count = 0;
    while count < 100 {
        playout(&mut rng, |board| {
            if count >= 100 {
                return;
            }
            let depth = (count % 4 + 1) as u32;
            let mut ctx = Ctx::new(&config);
            assert_eq!(
                negamax(board, depth, -INF, INF, &mut ctx),
                Some(minimax(board, depth, &config)),
                "{count}: {board:?} depth {depth}"
            );
            count += 1;
        });
    }
}

#[test]
fn caller_roots() {
    let config = config();
    let pass_root = Board {
        me: at(1, 0),
        opp: at(0, 0) | at(2, 0),
    };
    assert_eq!(legal_moves(pass_root.me, pass_root.opp), 0);
    let result = search(pass(pass_root), 2, &config);
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        -result.score,
        negamax(pass_root, 2, -INF, INF, &mut ctx).unwrap()
    );

    let terminal = Board {
        me: at(0, 0),
        opp: at(7, 7),
    };
    assert!(is_terminal(terminal));
    assert_eq!(legal_moves(terminal.me, terminal.opp), 0);
}

#[test]
fn depth_zero_fallback() {
    let config = config();
    let result = search(Board::START, 0, &config);
    assert_eq!(result.best, sq(3, 2));
    assert_eq!(result.score, evaluate(Board::START));
    assert_eq!(result.depth, 0);
    assert_eq!(result.nodes, 0);
    assert_eq!(result.status, SearchStatus::Fallback);
    assert!(!result.solved);
}

#[test]
fn complete_depth() {
    let config = config();
    let result = search(Board::START, 3, &config);
    let mut ctx = Ctx::new(&config);
    let expected = negamax(Board::START, 3, -INF, INF, &mut ctx).unwrap();
    assert_eq!(result.score, expected);
    assert_eq!(result.depth, 3);
    assert_eq!(result.status, SearchStatus::Complete);
    assert!(!result.solved);
    assert_ne!(
        legal_moves(Board::START.me, Board::START.opp) & (1 << result.best),
        0
    );
    assert!(result.nodes > 0);
}
