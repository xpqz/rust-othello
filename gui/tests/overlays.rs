use othello::board::{Board, legal_moves, play, squares};
use othello_gui::overlays::{NO_MOVE, frontier, opponent_move_counts};

const FORCED_PASS_ME: u64 = 0xe2e363232cd80009;
const FORCED_PASS_OPP: u64 = 0x081c18dcd227ff00;
const FORCED_PASS_MOVE: u8 = 47;

fn at(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

#[test]
fn start_move_counts() {
    let board = Board::START;
    let moves = legal_moves(board.me, board.opp);
    assert_eq!(moves.count_ones(), 4);
    let counts = opponent_move_counts(board);
    for square in squares(moves) {
        assert_eq!(counts[square as usize], 3, "{square}");
    }
    assert_eq!(counts.iter().filter(|&&count| count != NO_MOVE).count(), 4);
}

#[test]
fn forced_pass_is_zero() {
    let board = Board {
        me: FORCED_PASS_ME,
        opp: FORCED_PASS_OPP,
    };
    let counts = opponent_move_counts(board);
    assert_eq!(counts[FORCED_PASS_MOVE as usize], 0);
    let after = play(board, FORCED_PASS_MOVE);
    assert_eq!(legal_moves(after.me, after.opp), 0);
    assert_ne!(legal_moves(after.opp, after.me), 0);
}

#[test]
fn start_frontier() {
    let board = Board::START;
    assert_eq!(frontier(board).count_ones(), 4);
    assert_eq!(frontier(board), board.me | board.opp);
}

#[test]
fn surrounded_disc_is_not_frontier() {
    let center = 1u64 << 27;
    let opp = at(2, 2) | at(3, 2) | at(4, 2) | at(2, 3) | at(4, 3) | at(2, 4) | at(3, 4) | at(4, 4);
    let board = Board { me: center, opp };
    assert_eq!(frontier(board) & center, 0);
    assert_eq!(frontier(board), opp);
}

#[test]
fn white_to_move_is_relative() {
    let board = play(Board::START, 26);
    let moves = legal_moves(board.me, board.opp);
    let counts = opponent_move_counts(board);
    for square in squares(moves) {
        let after = play(board, square);
        assert_eq!(
            counts[square as usize],
            legal_moves(after.me, after.opp).count_ones() as u8,
            "{square}"
        );
    }
    assert_eq!(
        counts.iter().filter(|&&count| count != NO_MOVE).count() as u32,
        moves.count_ones()
    );
    assert_eq!(
        frontier(board),
        frontier(Board {
            me: board.opp,
            opp: board.me,
        })
    );
}

#[test]
fn edge_wrap_is_not_frontier() {
    let h4 = 1u64 << 31;
    let a4 = 1u64 << 24;
    let h4_opp = at(6, 2) | at(7, 2) | at(6, 3) | at(6, 4) | at(7, 4);
    assert_eq!(at(0, 4) & h4_opp, 0);
    let board = Board {
        me: h4,
        opp: h4_opp,
    };
    assert_eq!(frontier(board) & h4, 0);

    let a4_opp = at(1, 2) | at(0, 2) | at(1, 3) | at(1, 4) | at(0, 4);
    assert_eq!(at(7, 2) & a4_opp, 0);
    let board = Board {
        me: a4,
        opp: a4_opp,
    };
    assert_eq!(frontier(board) & a4, 0);
}
