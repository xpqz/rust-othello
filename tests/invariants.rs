mod common;

use common::{Naive, format_bits};
use othello::board::{disc_count, flips, is_terminal, legal_moves, pass, play};
use othello::rng::Rng;

#[test]
fn start_position() {
    let naive = Naive::start();
    let (me, opp) = naive.bits();
    assert_eq!(me, (1 << 28) | (1 << 35));
    assert_eq!(opp, (1 << 27) | (1 << 36));
    assert_eq!(
        format_bits(me, opp),
        "........\n........\n........\n...XO...\n...OX...\n........\n........\n........\n"
    );
}

#[test]
fn random_invariants() {
    let mut rng = Rng::new(0x1A5E);
    let mut positions = 0;
    for _ in 0..100 {
        common::playout(&mut rng, |board| {
            positions += 1;
            assert_eq!(board.me & board.opp, 0);
            let moves = legal_moves(board.me, board.opp);
            let mut rest = moves;
            while rest != 0 {
                let square = rest.trailing_zeros() as u8;
                rest &= rest - 1;
                assert_ne!(flips(board, square), 0);
                let after = play(board, square);
                assert_eq!(
                    disc_count(after.me) + disc_count(after.opp),
                    disc_count(board.me) + disc_count(board.opp) + 1
                );
                assert_eq!(after.me & after.opp, 0);
            }
            if moves == 0 && !is_terminal(board) {
                let after_pass = pass(board);
                assert_ne!(legal_moves(after_pass.me, after_pass.opp), 0);
            }
        });
    }
    assert!(positions > 5000, "{positions}");
}
