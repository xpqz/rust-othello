use crate::board::{Board, legal_moves, pass, play};

pub fn perft(board: Board, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = legal_moves(board.me, board.opp);
    if moves == 0 {
        return match legal_moves(board.opp, board.me) {
            0 => 1,
            _ => perft(pass(board), depth - 1),
        };
    }
    let mut nodes = 0;
    let mut rest = moves;
    while rest != 0 {
        nodes += perft(play(board, rest.trailing_zeros() as u8), depth - 1);
        rest &= rest - 1;
    }
    nodes
}
