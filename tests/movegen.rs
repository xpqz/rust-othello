use othello::board::{Board, Dir, legal_moves, shift};

fn bit(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

#[test]
fn shift_edges() {
    assert_eq!(shift(bit(0, 0), Dir::E), bit(1, 0));
    assert_eq!(shift(bit(7, 0), Dir::E), 0);
    assert_eq!(shift(bit(1, 0), Dir::W), bit(0, 0));
    assert_eq!(shift(bit(0, 0), Dir::W), 0);
    assert_eq!(shift(bit(0, 0), Dir::N), bit(0, 1));
    assert_eq!(shift(bit(0, 7), Dir::N), 0);
    assert_eq!(shift(bit(0, 1), Dir::S), bit(0, 0));
    assert_eq!(shift(bit(0, 0), Dir::S), 0);
    assert_eq!(shift(bit(0, 0), Dir::NE), bit(1, 1));
    assert_eq!(shift(bit(7, 0), Dir::NE), 0);
    assert_eq!(shift(bit(0, 0), Dir::NW), 0);
    assert_eq!(shift(bit(1, 1), Dir::SW), bit(0, 0));
    assert_eq!(shift(bit(0, 6), Dir::SE), bit(1, 5));
    assert_eq!(shift(bit(7, 7), Dir::SE), 0);
    assert_eq!(shift(bit(1, 5), Dir::NW), bit(0, 6));
    assert_eq!(shift(bit(0, 7), Dir::SW), 0);
}

#[test]
fn shift_masks() {
    assert_eq!(shift(u64::MAX, Dir::E).count_ones(), 56);
    assert_eq!(shift(u64::MAX, Dir::W).count_ones(), 56);
    assert_eq!(shift(u64::MAX, Dir::N).count_ones(), 56);
    assert_eq!(shift(u64::MAX, Dir::S).count_ones(), 56);
    assert_eq!(shift(u64::MAX, Dir::NE).count_ones(), 49);
    assert_eq!(shift(u64::MAX, Dir::NW).count_ones(), 49);
    assert_eq!(shift(u64::MAX, Dir::SE).count_ones(), 49);
    assert_eq!(shift(u64::MAX, Dir::SW).count_ones(), 49);
}

#[test]
fn start_board() {
    let start = Board::START;
    assert_eq!(
        start,
        Board {
            me: bit(3, 4) | bit(4, 3),
            opp: bit(3, 3) | bit(4, 4),
        }
    );
    let copy = start;
    assert_eq!(start, copy);
}

#[test]
fn start_moves() {
    let start = Board::START;
    let expected = bit(2, 3) | bit(3, 2) | bit(4, 5) | bit(5, 4);
    assert_eq!(legal_moves(start.me, start.opp), expected);
}

#[test]
fn legal_fixtures() {
    assert_eq!(legal_moves(0, 0), 0);

    let six_run = bit(1, 0) | bit(2, 0) | bit(3, 0) | bit(4, 0) | bit(5, 0) | bit(6, 0);
    assert_eq!(legal_moves(bit(0, 0), six_run), bit(7, 0));

    assert_eq!(legal_moves(bit(0, 0), bit(1, 1)), bit(2, 2));
    assert_eq!(legal_moves(bit(7, 0), bit(6, 1)), bit(5, 2));
    assert_eq!(legal_moves(bit(0, 7), bit(1, 6)), bit(2, 5));
    assert_eq!(legal_moves(bit(7, 7), bit(6, 6)), bit(5, 5));

    assert_eq!(legal_moves(bit(7, 0), bit(0, 1)), 0);
    assert_eq!(legal_moves(bit(0, 0) | bit(2, 0), bit(1, 0)), 0);
    assert_eq!(legal_moves(bit(0, 0), bit(1, 0) | bit(3, 0)), bit(2, 0));
}
