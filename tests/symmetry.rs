mod common;

use common::{Sym, transform};

fn at(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

#[test]
fn transform_orientation() {
    let b1 = at(1, 0);
    assert_eq!(Sym::ALL.len(), 8);
    assert_eq!(transform(b1, Sym::Identity), at(1, 0));
    assert_eq!(transform(b1, Sym::Rot90), at(7, 1));
    assert_eq!(transform(b1, Sym::Rot180), at(6, 7));
    assert_eq!(transform(b1, Sym::Rot270), at(0, 6));
    assert_eq!(transform(b1, Sym::FlipH), at(6, 0));
    assert_eq!(transform(b1, Sym::FlipV), at(1, 7));
    assert_eq!(transform(b1, Sym::FlipDiag), at(0, 1));
    assert_eq!(transform(b1, Sym::FlipAnti), at(7, 6));
}
