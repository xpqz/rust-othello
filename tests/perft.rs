use othello::board::Board;
use othello::perft::perft;

const START_COUNTS: [u64; 8] = [4, 12, 56, 244, 1396, 8200, 55092, 390216];

#[test]
fn perft_start_shallow() {
    for (i, expected) in START_COUNTS.iter().enumerate() {
        let depth = i as u32 + 1;
        assert_eq!(perft(Board::START, depth), *expected, "depth {depth}");
    }
}

#[test]
#[ignore]
fn perft_start_deep() {
    assert_eq!(perft(Board::START, 9), 3_005_288);
    assert_eq!(perft(Board::START, 10), 24_571_284);
}

#[test]
fn perft_zero_is_one() {
    let full = Board {
        me: (1 << 33) - 1,
        opp: !((1 << 33) - 1),
    };
    assert_eq!(perft(Board::START, 0), 1);
    assert_eq!(perft(full, 0), 1);
}

#[test]
fn perft_pass_consumes_ply() {
    let board = Board {
        me: 1 << 9,              // b2
        opp: (1 << 0) | (1 << 2), // a1, c1
    };
    assert_eq!(perft(board, 1), 1, "the pass position is a node");
    assert_eq!(perft(board, 2), 2, "both opponent replies");
}

#[test]
fn perft_early_terminal_counts_one() {
    let board = Board { me: 0x55, opp: 0xaa };
    assert_eq!(perft(board, 1), 1);
    assert_eq!(perft(board, 5), 1);
}
