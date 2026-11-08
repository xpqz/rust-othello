use crate::board::{Board, legal_moves, pass, play, squares};

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
    for square in squares(moves) {
        nodes += perft(play(board, square), depth - 1);
    }
    nodes
}
