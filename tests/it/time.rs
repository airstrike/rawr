#![cfg(not(feature = "no_time"))]
use rawr::{Engine, INT};
#[cfg(not(feature = "no_float"))]
use rawr::FLOAT;

#[test]
fn test_timestamp() {
    let engine = Engine::new();

    #[cfg(not(feature = "no_float"))]
    assert!(
        engine
            .eval::<FLOAT>(
                r#"
                    let time = std::time::Instant::now();
                    let mut x = 10_000;
                    while x > 0 { x -= 1; }
                    time.elapsed().as_secs_f64()
                "#
            )
            .unwrap()
            < 10.0
    );

    assert!(
        engine
            .eval::<INT>(
                r#"
                    let time = std::time::Instant::now();
                    let mut x = 10_000;
                    while x > 0 { x -= 1; }
                    time.elapsed().as_secs() as i64
                "#
            )
            .unwrap()
            < 10
    );

    assert!(
        engine
            .eval::<bool>(
                "
                use std::time::Instant;

                let time1 = Instant::now();
                for x in 0..10000 {}
                let time2 = Instant::now();
                time1 <= time2
            "
            )
            .unwrap()
    );
}

#[test]
fn test_timestamp_op() {
    let engine = Engine::new();

    #[cfg(not(feature = "no_float"))]
    assert!(
        (engine
            .eval::<FLOAT>(
                r#"
                    use std::time::{Duration, Instant};

                    let time1 = Instant::now();
                    let time2 = time1 + Duration::from_secs_f64(123.45);
                    (time2 - time1).as_secs_f64()
                "#
            )
            .unwrap()
            - 123.45)
            .abs()
            < 0.001
    );

    #[cfg(not(feature = "no_float"))]
    assert!(
        (engine
            .eval::<FLOAT>(
                r#"
                    use std::time::{Duration, Instant};

                    let time1 = Instant::now();
                    let time2 = time1 - Duration::from_secs_f64(123.45);
                    (time1 - time2).as_secs_f64()
                "#
            )
            .unwrap()
            - 123.45)
            .abs()
            < 0.001
    );

    assert_eq!(
        engine
            .eval::<INT>(
                r#"
                    use std::time::{Duration, Instant};

                    let time1 = Instant::now();
                    let time2 = time1 + Duration::from_secs(42);
                    (time2 - time1).as_secs() as i64
                "#
            )
            .unwrap(),
        42
    );

    assert_eq!(
        engine
            .eval::<INT>(
                r#"
                    use std::time::{Duration, Instant};

                    let time1 = Instant::now();
                    let time2 = time1 - Duration::from_secs(42);
                    time1.duration_since(time2).as_secs() as i64
                "#
            )
            .unwrap(),
        42
    );
}
