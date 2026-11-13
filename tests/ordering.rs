mod common;

use common::playout;
use othello::board::{Board, legal_moves, play, squares};
use othello::rng::Rng;
use othello::search::ordered_moves;
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
