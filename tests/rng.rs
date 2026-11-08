use othello::rng::Rng;

#[test]
fn known_vectors() {
    let mut rng = Rng::new(1);
    assert_eq!(rng.next_u64(), 0x47E4_CE4B_896C_DD1D);
    assert_eq!(rng.next_u64(), 0xABCF_A6A8_E079_651D);
    assert_eq!(rng.next_u64(), 0xB9D1_0D8F_EB73_1F57);

    let mut zero = Rng::new(0);
    assert_eq!(zero.next_u64(), 0x0D83_B3E2_9A21_487A);
}
