use othello::board::Board;
use othello::perft::perft;

const START_COUNTS: [u64; 8] = [4, 12, 56, 244, 1396, 8200, 55092, 390216];

fn bit(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

#[test]
fn perft_start_shallow() {
    for (i, expected) in START_COUNTS.iter().enumerate() {
        let depth = i as u32 + 1;
        assert_eq!(perft(Board::START, depth), *expected, "depth {depth}");
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release-only")]
fn perft_start_deep() {
    assert_eq!(perft(Board::START, 9), 3_005_288);
    assert_eq!(perft(Board::START, 10), 24_571_284);
}

#[test]
fn perft_zero_is_one() {
    let me = (1u64 << 33) - 1;
    let full = Board { me, opp: !me };
    assert_eq!(perft(Board::START, 0), 1);
    assert_eq!(perft(full, 0), 1);
}

#[test]
fn perft_pass_consumes_ply() {
    let board = Board {
        me: bit(1, 1),
        opp: bit(0, 0) | bit(2, 0),
    };
    assert_eq!(perft(board, 1), 1, "the pass position is a node");
    assert_eq!(perft(board, 2), 2, "both opponent replies");
}

#[test]
fn perft_early_terminal_counts_one() {
    let board = Board {
        me: bit(0, 0) | bit(2, 0) | bit(4, 0) | bit(6, 0),
        opp: bit(1, 0) | bit(3, 0) | bit(5, 0) | bit(7, 0),
    };
    assert_eq!(perft(board, 1), 1);
    assert_eq!(perft(board, 5), 1);
}
