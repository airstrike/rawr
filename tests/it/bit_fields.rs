use rawr::{Engine, INT};

#[test]
fn test_left_shift() {
    let engine = Engine::new();
    assert_eq!(engine.eval::<INT>("4 << 2").unwrap(), 16);
}

#[test]
fn test_right_shift() {
    let engine = Engine::new();
    assert_eq!(engine.eval::<INT>("9 >> 1").unwrap(), 4);
}

#[test]
fn test_bit_fields() {
    let engine = Engine::new();

    // Rust reads and writes bits with shifts and masks.
    assert!(!engine.eval::<bool>("let x: i64 = 10; (x >> 0) & 1 == 1").unwrap());
    assert!(engine.eval::<bool>("let x: i64 = 10; (x >> 1) & 1 == 1").unwrap());
    assert!(!engine.eval::<bool>("let x: i64 = 10; (x >> 63) & 1 == 1").unwrap());
    assert_eq!(engine.eval::<INT>("let mut x: i64 = 10; x |= 1; x &= !(1 << 1); x").unwrap(), 9);
    assert_eq!(engine.eval::<INT>("let x: i64 = 10; (x >> 1) & 0b111").unwrap(), 5);
    assert_eq!(engine.eval::<INT>("let mut x: i64 = 10; x = (x & !(0b111 << 1)) | (7 << 1); x").unwrap(), 14);
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x: i64 = 0b001101101010001;
                    let mut count = 0;

                    for i in 2..12 {
                        if (x >> i) & 1 == 1 { count += 1; }
                    }

                    count
                "
            )
            .unwrap(),
        5
    );
    assert_eq!(engine.eval::<INT>("let x: i64 = 0b001101101010001; ((x >> 2) & 0x3ff).count_ones() as i64").unwrap(), 5);

    // Indexing an integer is not Rust.
    let _ = engine.compile("let x = 10; x[0]").unwrap_err();
}
