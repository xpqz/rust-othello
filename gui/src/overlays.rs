use othello::board::{Board, Dir, legal_moves, play, shift, squares};

pub const NO_MOVE: u8 = 255;

pub fn opponent_move_counts(board: Board) -> [u8; 64] {
    let mut counts = [NO_MOVE; 64];
    for square in squares(legal_moves(board.me, board.opp)) {
        let after = play(board, square);
        counts[square as usize] = legal_moves(after.me, after.opp).count_ones() as u8;
    }
    counts
}

pub fn frontier(board: Board) -> u64 {
    let empty = !(board.me | board.opp);
    let touching = Dir::ALL
        .iter()
        .fold(0, |bits, &dir| bits | shift(empty, dir));
    touching & (board.me | board.opp)
}
