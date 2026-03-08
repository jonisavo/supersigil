use supersigil_rust::verifies;

#[verifies("req/validation#crit-1")]
proptest! {
    fn test_roundtrip(input in ".*") {
        let encoded = encode(&input);
        let decoded = decode(&encoded);
        prop_assert_eq!(input, decoded);
    }
}
