mod common;

use common::{Sym, transform};

fn at(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

#[test]
fn transform_orientation() {
    assert_eq!(Sym::ALL.len(), 8);
    assert_eq!(transform(at(0, 0), Sym::Rot90), at(7, 0));
    assert_eq!(transform(at(0, 0), Sym::Rot180), at(7, 7));
    assert_eq!(transform(at(0, 0), Sym::Rot270), at(0, 7));
    assert_eq!(transform(at(0, 0), Sym::FlipH), at(7, 0));
    assert_eq!(transform(at(0, 0), Sym::FlipV), at(0, 7));
    assert_eq!(transform(at(0, 0), Sym::FlipDiag), at(0, 0));
    assert_eq!(transform(at(0, 0), Sym::FlipAnti), at(7, 7));
    assert_eq!(transform(at(1, 0), Sym::Rot90), at(7, 1));
}
