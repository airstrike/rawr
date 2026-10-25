use rawr::{Engine, INT};

#[test]
fn test_for_loop() {
    let engine = Engine::new();

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut sum1 = 0;
                    let mut sum2 = 0;
                    let inputs = vec![1, 2, 3, 4, 5];

                    for x in inputs {
                        sum1 += x;
                    }

                    for x in 1..6 {
                        sum2 += x;
                    }

                    for x in (1..6).step_by(3) {
                        sum2 += x;
                    }

                    sum1 + sum2
                "
            )
            .unwrap(),
        35
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut sum = 0;
                    let inputs = vec![1, 2, 3, 4, 5];

                    for (i, x) in inputs.iter().enumerate() {
                        sum += x * (i as i64 + 1);
                    }
                    sum
                "
            )
            .unwrap(),
        55
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut sum = 0;
                    for x in 1..10 { sum += x; }
                    sum
                "
            )
            .unwrap(),
        45
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut sum = 0;
                    for x in 1..=10 { sum += x; }
                    sum
                "
            )
            .unwrap(),
        55
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut sum = 0;
                    for x in (1..10).step_by(2) { sum += x; }
                    sum
                "
            )
            .unwrap(),
        25
    );

    // A range whose start is past its end is empty.
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut sum = 0;
                    for x in (10..1).step_by(2) { sum += x; }
                    sum
                "
            )
            .unwrap(),
        0
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut sum = 0;
                    for x in (2..=10).rev().step_by(2) { sum += x; }
                    sum
                "
            )
            .unwrap(),
        30
    );

    // `break` leaves the loop early.
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let a = vec![123, 999, 42, 0, 7];
                    let mut found = 0;

                    for (count, item) in a.iter().enumerate() {
                        if item % 2 == 0 {
                            found = count;
                            break;
                        }
                    }
                    found
                "
            )
            .unwrap(),
        2
    );
}

// Stepping past the end of the integer range stops the loop rather than
// overflowing.
#[test]
fn test_for_overflow() {
    let engine = Engine::new();

    let script = "
        let mut sum = 0;

        for x in (i64::MAX - 2..=i64::MAX).step_by(2) {
            sum += 1;
        }

        sum
    ";

    assert_eq!(engine.eval::<INT>(script).unwrap(), 2);
}

#[test]
fn test_for_string() {
    let engine = Engine::new();

    assert_eq!(
        engine
            .eval::<INT>(
                r#"
                    let s = "hello";
                    let mut sum = 0;

                    for ch in s.chars() {
                        sum += ch as i64;
                    }

                    sum
                "#
            )
            .unwrap(),
        532
    );

    assert_eq!(
        engine
            .eval::<INT>(
                r#"
                    let s = "hello";
                    let mut sum = 0;

                    for ch in s[2..=3].chars() {
                        sum += ch as i64;
                    }

                    sum
                "#
            )
            .unwrap(),
        216
    );
}

#[test]
fn test_for_object() {
    let engine = Engine::new();

    let script = r#"
        use std::collections::HashMap;

        let mut sum = 0;
        let mut keys = String::new();
        let map = HashMap::from([("a", 1), ("b", 2), ("c", 3)]);

        for key in map.keys() {
            keys += key;
        }
        for value in map.values() {
            sum += value;
        }

        keys.len() as i64 + sum
    "#;

    assert_eq!(engine.eval::<INT>(script).unwrap(), 9);
}

#[test]
fn test_for_capture() {
    let engine = Engine::new();

    // Each closure moves in its own loop values.
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut a: Vec<Box<dyn Fn() -> i64>> = Vec::new();

                    for (i, x) in (100..110).enumerate() {
                        a.push(Box::new(move || i as i64 + x));
                    }

                    let mut sum = 0;

                    for fp in a {
                        sum += fp();
                    }

                    sum
                "
            )
            .unwrap(),
        1090
    );
}
