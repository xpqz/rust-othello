use othello::board::Board;

fn apply(bits: u64, kind: u8) -> u64 {
    let mut out = 0;
    let mut rest = bits;
    while rest != 0 {
        let square = rest.trailing_zeros() as usize;
        rest &= rest - 1;
        let (file, rank) = (square % 8, square / 8);
        let (file, rank) = match kind {
            0 => (file, rank),
            1 => (7 - rank, file),
            2 => (7 - file, 7 - rank),
            3 => (rank, 7 - file),
            4 => (7 - file, rank),
            5 => (file, 7 - rank),
            6 => (rank, file),
            _ => (7 - rank, 7 - file),
        };
        out |= 1u64 << (rank * 8 + file);
    }
    out
}

pub fn transform(board: Board, kind: u8) -> Board {
    Board {
        me: apply(board.me, kind),
        opp: apply(board.opp, kind),
    }
}

pub fn canonical(board: Board) -> (u64, u64) {
    (0..8)
        .map(|kind| {
            let board = transform(board, kind);
            (board.me, board.opp)
        })
        .min()
        .unwrap()
}
