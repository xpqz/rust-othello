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
    assert_eq!(shift(bit(0, 1), Dir::S), bit(0, 0));
    assert_eq!(shift(bit(0, 0), Dir::S), 0);
    assert_eq!(shift(bit(0, 7), Dir::N), 0);
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
fn start_board() {
    let start = Board::start();
    assert_eq!(
        start,
        Board {
            me: (1 << 28) | (1 << 35),
            opp: (1 << 27) | (1 << 36),
        }
    );
    let copy = start;
    assert_eq!(start, copy);
}

#[test]
fn start_moves() {
    let start = Board::start();
    let expected = bit(2, 3) | bit(3, 2) | bit(4, 5) | bit(5, 4);
    assert_eq!(legal_moves(start.me, start.opp), expected);
}

#[test]
fn line_moves() {
    let me = bit(2, 0);
    let opp = bit(1, 0);
    assert_eq!(legal_moves(me, opp), bit(0, 0));
    assert_eq!(legal_moves(0, 0), 0);
}
