mod common;

use common::{Factory, Naive, Sym, format_bits, seeded, transform};

#[test]
fn common_helpers() {
    let mut a = seeded(0);
    let mut b = seeded(0);
    let first = a.next_u64();
    assert_ne!(first, 0);
    assert_eq!(first, b.next_u64());

    let naive = Naive::start();
    let (me, opp) = naive.bits();
    assert_eq!(me, (1 << 28) | (1 << 35));
    assert_eq!(opp, (1 << 27) | (1 << 36));
    assert_eq!(format_bits(me, opp).lines().count(), 8);

    for sym in [
        Sym::Identity,
        Sym::Rot90,
        Sym::Rot180,
        Sym::Rot270,
        Sym::FlipH,
        Sym::FlipV,
        Sym::FlipDiag,
        Sym::FlipAnti,
    ] {
        let _ = transform(me | opp, sym);
    }

    let mut factory = Factory::new(|| seeded(7));
    assert_eq!(factory.spawn().next_u64(), seeded(7).next_u64());
}
