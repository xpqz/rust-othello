mod common;

use common::playout;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use othello::board::{Board, empty_count, legal_moves, play, squares};
use othello::rng::Rng;
use othello::score::INF;
use othello::search::{Ctx, SearchConfig, negamax, ordered_moves, search, solve};
use othello::tt::zobrist;

fn noise(board: Board) -> i32 {
    ((zobrist(board) >> 20) % 4001) as i32 - 2000
}

/// Given an evaluation, moves come in ascending order of the child's value to
/// the opponent plus 300 per opponent reply, the preferred move counted 800
/// better, ties by square.
#[test]
fn evaluation_order() {
    let mut rng = Rng::new(0x0D);
    let (mut visited, mut count, mut led, mut trailed) = (0, 0, 0, 0);
    while count < 200 {
        playout(&mut rng, |board| {
            visited += 1;
            let moves = legal_moves(board.me, board.opp);
            if visited % 5 != 0 || count >= 200 || moves.count_ones() < 3 {
                return;
            }
            let hints = squares(moves).map(Some).chain([None]);
            for preferred in hints {
                let mut expected: Vec<(i32, u8)> = squares(moves)
                    .map(|square| {
                        let child = play(board, square);
                        let replies = legal_moves(child.me, child.opp).count_ones() as i32;
                        let bonus = if Some(square) == preferred { 800 } else { 0 };
                        (noise(child) + 300 * replies - bonus, square)
                    })
                    .collect();
                expected.sort();
                let expected: Vec<u8> = expected.iter().map(|&(_, square)| square).collect();
                let (order, len) = ordered_moves(board, moves, 4, preferred, Some(noise));
                assert_eq!(&order[..len], &expected[..], "{board:?} {preferred:?}");
                match preferred {
                    Some(square) if order[0] == square => led += 1,
                    Some(_) => trailed += 1,
                    None => {}
                }
            }
            count += 1;
        });
    }
    assert!(led > 100 && trailed > 100, "{led} led, {trailed} trailed");
}

fn flat(_: Board) -> i32 {
    0
}

/// Equal keys fall back to square order; the discount breaks the tie.
#[test]
fn evaluation_ties() {
    let board = Board::START;
    let moves = legal_moves(board.me, board.opp);
    let (order, len) = ordered_moves(board, moves, 4, None, Some(flat));
    assert_eq!(&order[..len], &[19, 26, 37, 44]);
    let (order, len) = ordered_moves(board, moves, 4, Some(37), Some(flat));
    assert_eq!(&order[..len], &[37, 19, 26, 44]);
}

static EMPTIES: AtomicU64 = AtomicU64::new(0);

fn recording(board: Board) -> i32 {
    EMPTIES.fetch_or(1 << empty_count(board), Relaxed);
    noise(board)
}

fn evaluated(run: impl FnOnce(&SearchConfig)) -> u64 {
    let config = SearchConfig {
        eval: recording,
        endgame_threshold: 0,
        ordering: true,
    };
    EMPTIES.store(0, Relaxed);
    run(&config);
    EMPTIES.load(Relaxed)
}

/// Which positions get evaluated shows where ordering by evaluation is on:
/// interior midgame nodes with four or more plies left, and nowhere else.
#[test]
fn evaluation_boundary() {
    let board = Board {
        me: 0x2002_242e_1000_0000,
        opp: 0x023c_1810_2840_0000,
    };
    let empties = empty_count(board);
    let at = |plies: &[u32]| {
        plies
            .iter()
            .fold(0u64, |set, ply| set | 1 << (empties - ply))
    };
    let deep = |depth| {
        evaluated(|config| {
            negamax(board, depth, -INF, INF, &mut Ctx::new(config)).unwrap();
        })
    };
    assert_eq!(deep(3), at(&[3]));
    assert_eq!(deep(4), at(&[1, 4]));
    assert_eq!(deep(6), at(&[1, 2, 3, 6]));
    let rooted = evaluated(|config| {
        search(board, 4, config);
    });
    assert_eq!(rooted, at(&[0, 4]));
    let unordered = evaluated(|config| {
        let config = SearchConfig {
            ordering: false,
            ..*config
        };
        negamax(board, 5, -INF, INF, &mut Ctx::new(&config)).unwrap();
    });
    assert_eq!(unordered, at(&[5]));
    let late = Board {
        me: 0x00c8_dc1c_1c0e_44bc,
        opp: 0x3f37_2343_8371_bb01,
    };
    let solved = evaluated(|config| {
        solve(late, -INF, INF, &mut Ctx::new(config)).unwrap();
    });
    assert_eq!(solved, 0);
}

const CORNERS: u64 = 0x8100_0000_0000_0081;
const XC: u64 = 0x42c3_0000_0000_c342;

/// Without an evaluation and under depth 2 the order is preferred move,
/// corners, other squares, X and C squares, each by square.
#[test]
fn class_order() {
    let mut rng = Rng::new(0xC1);
    let (mut visited, mut count) = (0, 0);
    while count < 1000 {
        playout(&mut rng, |board| {
            visited += 1;
            let moves = legal_moves(board.me, board.opp);
            if visited % 3 != 0 || count >= 1000 || moves == 0 {
                return;
            }
            for preferred in squares(moves).map(Some).chain([None]) {
                let mut expected: Vec<(u32, u8)> = squares(moves)
                    .map(|square| {
                        let class = match (CORNERS >> square & 1, XC >> square & 1) {
                            _ if Some(square) == preferred => 0,
                            (1, _) => 1,
                            (0, 1) => 3,
                            _ => 2,
                        };
                        (class, square)
                    })
                    .collect();
                expected.sort();
                let expected: Vec<u8> = expected.iter().map(|&(_, square)| square).collect();
                for depth in [0, 1] {
                    let (order, len) = ordered_moves(board, moves, depth, preferred, None);
                    assert_eq!(&order[..len], &expected[..], "{board:?} {preferred:?}");
                }
            }
            count += 1;
        });
    }
}

/// No moves give an empty order, and a preferred square that is not among the
/// moves is not added.
#[test]
fn class_edges() {
    let board = Board::START;
    let moves = legal_moves(board.me, board.opp);
    for depth in [0, 1] {
        assert_eq!(ordered_moves(board, 0, depth, Some(19), None).1, 0);
        let (order, len) = ordered_moves(board, moves, depth, Some(0), None);
        assert_eq!(&order[..len], &[19, 26, 37, 44]);
    }
}
