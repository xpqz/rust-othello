mod common;

use common::{Naive, format_bits};

#[test]
fn start_position() {
    let naive = Naive::start();
    let (me, opp) = naive.bits();
    assert_eq!(me, (1 << 28) | (1 << 35));
    assert_eq!(opp, (1 << 27) | (1 << 36));
    assert_eq!(
        format_bits(me, opp),
        "........\n........\n........\n...XO...\n...OX...\n........\n........\n........\n"
    );
}
