#![cfg(not(feature = "no_index"))]
use rawr::{Array, Dynamic, Engine, INT, ParseErrorType};
use std::iter::FromIterator;

#[test]
fn test_arrays() {
    let a = Array::from_iter([(42 as INT).into()]);

    assert_eq!(a[0].clone_cast::<INT>(), 42);

    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("let x = [1, 2, 3]; x[1]").unwrap(), 2);
    assert_eq!(engine.eval::<INT>("let x = vec![1, 2, 3,]; x[1]").unwrap(), 2);
    assert_eq!(engine.eval::<INT>("let mut y = vec![1, 2, 3]; y[1] = 5; y[1]").unwrap(), 5);
    assert_eq!(engine.eval::<INT>("let y = vec![vec![1, 2, 3], vec![42, 88, 93]]; y[1][2]").unwrap(), 93);
    assert_eq!(engine.eval::<INT>("let y = vec![1, 2, 3]; y[0]").unwrap(), 1);
    assert_eq!(engine.eval::<INT>("let y = vec![1, 2, 3]; y[y.len() - 1]").unwrap(), 3);
    let _ = engine.eval::<INT>("let y: Vec<i64> = vec![]; y[0]").unwrap_err();
    let _ = engine.eval::<INT>("let y: Vec<i64> = vec![]; y[1]").unwrap_err();
    let _ = engine.eval::<INT>("let y = vec![1, 2, 3]; y[3]").unwrap_err();
    assert!(engine.eval::<bool>("let y = vec![1, 2, 3]; y.get(3).is_none()").unwrap());
    assert!(engine.eval::<bool>("let y = vec![1, 2, 3]; y.contains(&2)").unwrap());
    assert!(!engine.eval::<bool>("let y = vec![1, 2, 3]; y.contains(&42)").unwrap());
    assert_eq!(engine.eval::<INT>("let mut y = vec![1, 2, 3]; y.resize(5, 42); y.len()").unwrap(), 5);
    assert_eq!(engine.eval::<INT>("let mut y = vec![1, 2, 3]; y.resize(5, 42); y[4]").unwrap(), 42);
    assert_eq!(engine.eval::<Dynamic>("let mut y = vec![1, 2, 3]; y[1] += 4; y").unwrap().into_typed_array::<INT>().unwrap(), [1, 6, 3]);
    assert_eq!(engine.eval::<Dynamic>("let y = vec![1, 2, 3]; y[1..].to_vec()").unwrap().into_typed_array::<INT>().unwrap(), vec![2, 3]);
    assert_eq!(engine.eval::<Dynamic>("let y = vec![1, 2, 3]; y[..1].to_vec()").unwrap().into_typed_array::<INT>().unwrap(), vec![1]);
    assert_eq!(engine.eval::<Dynamic>("let mut y = vec![1, 2, 3]; y.truncate(1); y").unwrap().into_typed_array::<INT>().unwrap(), vec![1]);

    #[cfg(not(feature = "no_object"))]
    {
        assert_eq!(engine.eval::<Dynamic>("let mut y = vec![1, 2, 3]; y.push(4); y").unwrap().into_typed_array::<INT>().unwrap(), [1, 2, 3, 4]);
        // `insert` is Vec's: it takes a `usize` index up to the length.
        assert_eq!(engine.eval::<Dynamic>("let mut y = vec![1, 2, 3]; y.insert(0, 4); y").unwrap().into_typed_array::<INT>().unwrap(), [4, 1, 2, 3]);
        assert_eq!(engine.eval::<Dynamic>("let mut y = vec![1, 2, 3]; y.insert(3, 4); y").unwrap().into_typed_array::<INT>().unwrap(), [1, 2, 3, 4]);
        let _ = engine.eval::<Dynamic>("let mut y = vec![1, 2, 3]; y.insert(999, 4); y").unwrap_err();
        assert_eq!(engine.eval::<INT>("let y = vec![1, 2, 3]; let z = vec![42]; y[z.len()]").unwrap(), 2);
        assert_eq!(engine.eval::<INT>("let y = vec![vec![1, 2], vec![3, 4, 5, 6]]; let z = vec![42]; y[1][z.len()]").unwrap(), 4);
        assert_eq!(engine.eval::<INT>("let y = vec![1, 2, 3]; let z = vec![2]; y[z[0]]").unwrap(), 3);

        assert_eq!(
            engine
                .eval::<Dynamic>(
                    "
                        let mut x = vec![2, 9];
                        x.insert(1, 1);
                        x.insert(3, 3);
                        x.insert(0, 99);

                        let r = x.remove(2);

                        x.extend(vec![4, 5]);

                        x
                    "
                )
                .unwrap()
                .into_typed_array::<INT>()
                .unwrap(),
            [99, 2, 9, 3, 4, 5]
        );
    }

    #[cfg(not(feature = "no_object"))]
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    struct Foo { foo: i64 }

                    let x = Foo { foo: 42 };
                    let n = 0;
                    let a = vec![vec![x]];
                    let i = vec![n];
                    a[n][i[n]].foo
                "
            )
            .unwrap(),
        42
    );

    assert_eq!(
        engine
            .eval::<Dynamic>(
                "
                    let mut x = vec![1, 2, 3];
                    let y = vec![4, 5];
                    x.extend_from_slice(&y);
                    x
                "
            )
            .unwrap()
            .into_typed_array::<INT>()
            .unwrap(),
        [1, 2, 3, 4, 5]
    );
    assert_eq!(
        engine
            .eval::<Dynamic>(
                "
                    let x = vec![1, 2, 3];
                    let y = vec![4, 5];
                    x.iter().chain(y.iter()).cloned().collect::<Vec<i64>>()
                "
            )
            .unwrap()
            .into_typed_array::<INT>()
            .unwrap(),
        [1, 2, 3, 4, 5]
    );
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x = 42;
                    let mut y = vec![];
                    for n in 0..10 {
                        y.push(x);
                    }
                    y.len()
                "
            )
            .unwrap(),
        10
    );

    let value = vec![String::from("hello"), String::from("world"), String::from("foo"), String::from("bar")];

    let array: Dynamic = value.into();

    assert_eq!(array.type_name(), "array");

    let array = array.cast::<Array>();

    assert_eq!(array[0].type_name(), "string");
    assert_eq!(array.len(), 4);
}

#[cfg(not(feature = "no_float"))]
#[cfg(not(feature = "no_object"))]
#[test]
fn test_array_chaining() {
    let engine = Engine::new();

    assert!(
        engine
            .eval::<bool>(
                "
                let v = vec![std::f64::consts::PI];
                ( v[0].cos() ).sin() == v[0].cos().sin()
            "
            )
            .unwrap()
    );
}

#[test]
fn test_array_index_types() {
    use rawr::resolve::Error;

    let engine = Engine::new();

    engine.compile("[1, 2, 3][0]").unwrap();

    // An index is a `usize`, so any other type is refused before the script runs.
    assert!(matches!(
        engine.compile("[1, 2, 3][0]['x']").unwrap_err().err_type(),
        ParseErrorType::Resolution(Error::TypeMismatch { expected, .. }) if expected == "usize"
    ));

    assert!(matches!(
        engine.compile("[1, 2, 3]['x']").unwrap_err().err_type(),
        ParseErrorType::Resolution(Error::TypeMismatch { expected, .. }) if expected == "usize"
    ));

    #[cfg(not(feature = "no_float"))]
    assert!(matches!(
        engine.compile("[1, 2, 3][123.456]").unwrap_err().err_type(),
        ParseErrorType::Resolution(Error::TypeMismatch { expected, .. }) if expected == "usize"
    ));

    assert!(matches!(
        engine.compile("[1, 2, 3][()]").unwrap_err().err_type(),
        ParseErrorType::Resolution(Error::TypeMismatch { expected, .. }) if expected == "usize"
    ));

    assert!(matches!(
        engine.compile(r#"[1, 2, 3]["hello"]"#).unwrap_err().err_type(),
        ParseErrorType::Resolution(Error::TypeMismatch { expected, .. }) if expected == "usize"
    ));

    assert!(matches!(
        engine.compile("[1, 2, 3][true && false]").unwrap_err().err_type(),
        ParseErrorType::Resolution(Error::TypeMismatch { expected, .. }) if expected == "usize"
    ));
}

#[cfg(not(feature = "no_object"))]
#[cfg(not(feature = "no_function"))]
#[cfg(not(feature = "no_closure"))]
#[test]
fn test_arrays_map_reduce() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("vec![1].iter().map(|x| x + 41).collect::<Vec<_>>()[0]").unwrap(), 42);
    assert_eq!(engine.eval::<INT>("let mut x = vec![1, 2, 3]; for v in x.iter_mut() { *v += 41; } x[0]").unwrap(), 42);
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x = vec![1, 2, 3];
                    let mut sum = 0;
                    let factor = 2;
                    x.iter().for_each(|v| sum += v * factor);
                    sum
                "
            )
            .unwrap(),
        12
    );
    assert_eq!(engine.eval::<INT>("(vec![1].iter().map(|x| x + 41).collect::<Vec<_>>())[0]").unwrap(), 42);
    assert_eq!(engine.eval::<INT>("let mut x = vec![1, 2, 3]; for (i, v) in x.iter_mut().enumerate() { *v += i as i64; } x[2]").unwrap(), 5);

    assert_eq!(
        engine
            .eval::<Dynamic>(
                "
                    let x = vec![1, 2, 3];
                    x.iter().filter(|v| **v > 2).cloned().collect::<Vec<i64>>()
                "
            )
            .unwrap()
            .into_typed_array::<INT>()
            .unwrap(),
        [3]
    );

    assert_eq!(
        engine
            .eval::<Dynamic>(
                "
                    let x = vec![1, 2, 3];
                    x.iter().enumerate().filter(|&(i, v)| *v > i as i64).map(|(_, v)| *v).collect::<Vec<i64>>()
                "
            )
            .unwrap()
            .into_typed_array::<INT>()
            .unwrap(),
        [1, 2, 3]
    );

    assert_eq!(
        engine
            .eval::<Dynamic>(
                "
                    let x = vec![1, 2, 3];
                    x.iter().map(|v| v * 2).collect::<Vec<_>>()
                "
            )
            .unwrap()
            .into_typed_array::<INT>()
            .unwrap(),
        [2, 4, 6]
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x = vec![1, 2, 3];
                    x.iter().fold(0, |sum, v| sum + v * v)
                "
            )
            .unwrap(),
        14
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x = vec![1, 2, 3];
                    x.iter().enumerate().fold(0, |sum, (i, v)| {
                        let start = if i == 0 { 10 } else { sum };
                        start + v * v
                    })
                "
            )
            .unwrap(),
        24
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x = vec![1, 2, 3];
                    x.iter().rev().fold(0, |sum, v| sum + v * v)
                "
            )
            .unwrap(),
        14
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x = vec![1, 2, 3];
                    x.iter().rev().enumerate().fold(0, |sum, (i, v)| {
                        let start = if i == 0 { 10 } else { sum };
                        start + v * v
                    })
                "
            )
            .unwrap(),
        24
    );

    assert!(
        engine
            .eval::<bool>(
                "
                let x = vec![1, 2, 3];
                x.iter().any(|v| *v > 1)
            "
            )
            .unwrap()
    );

    assert!(
        engine
            .eval::<bool>(
                "
                let x = vec![1, 2, 3];
                x.iter().enumerate().any(|(i, v)| v * i as i64 == 0)
            "
            )
            .unwrap()
    );

    assert!(
        !engine
            .eval::<bool>(
                "
                let x = vec![1, 2, 3];
                x.iter().all(|v| *v > 1)
            "
            )
            .unwrap()
    );

    assert!(
        engine
            .eval::<bool>(
                "
                let x = vec![1, 2, 3];
                x.iter().enumerate().all(|(i, v)| *v > i as i64)
            "
            )
            .unwrap()
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x = vec![1, 2, 3];
                    x.iter().find(|v| **v > 2).copied().unwrap()
                "
            )
            .unwrap(),
        3
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x = vec![1, 2, 3];
                    x.iter().enumerate().find(|&(i, v)| *v * i as i64 == 6).map(|(_, v)| *v).unwrap()
                "
            )
            .unwrap(),
        3
    );

    assert!(
        engine
            .eval::<bool>(
                "
                let x = vec![1, 2, 3, 2, 1];
                x.iter().find(|v| **v > 4).is_none()
            ",
            )
            .unwrap()
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x = vec![None, Some(2), None];
                    x.iter().find_map(|v| *v).unwrap()
                "
            )
            .unwrap(),
        2
    );

    assert!(
        engine
            .eval::<bool>(
                "
                let x: Vec<Option<i64>> = vec![Some(1), None];
                x.iter().find_map(|v| v.filter(|n| *n > 1)).is_none()
            ",
            )
            .unwrap()
    );
}

#[test]
fn test_arrays_index_of() {
    let engine = Engine::new();
    // An array of integers.
    assert_eq!(engine.eval::<INT>("vec![1, 2, 3].iter().position(|v| *v == 2).unwrap()").unwrap(), 1);
    // An array of strings.
    assert_eq!(engine.eval::<INT>(r#"vec!["hi", "hello", "world"].iter().position(|s| *s == "hello").unwrap()"#).unwrap(), 1);
    assert_eq!(engine.eval::<INT>(r#"vec!["a", "b", "c", "b"].iter().skip(2).position(|s| *s == "b").map(|p| p + 2).unwrap()"#).unwrap(), 3);
    assert!(engine.eval::<bool>(r#"vec!["a", "b"].iter().position(|s| *s == "missing").is_none()"#).unwrap());
}

#[test]
#[cfg(not(feature = "no_object"))]
#[cfg(not(feature = "no_function"))]
fn test_arrays_index_of_with_named_fn() {
    let engine = Engine::new();
    assert_eq!(
        engine
            .eval::<INT>(
                r#"
             fn is_two(x: i64) -> bool { x == 2 }
             vec![1, 2, 3, 2].into_iter().position(is_two).unwrap()
         "#
            )
            .unwrap(),
        1
    );
    assert_eq!(
        engine
            .eval::<INT>(
                r#"
             fn is_two(x: i64) -> bool { x == 2 }
             vec![1, 2, 3, 2].into_iter().skip(2).position(is_two).unwrap() + 2
         "#
            )
            .unwrap(),
        3
    );
}
#[test]
#[cfg(feature = "internals")]
fn test_array_invalid_index_callback() {
    use std::convert::TryInto;

    let mut engine = Engine::new();

    engine.on_invalid_array_index(|arr, index, _| match index {
        3 => {
            arr.push((42 as INT).into());
            arr.last_mut().unwrap().try_into()
        }
        100 => Ok(Dynamic::from(100 as INT).into()),
        _ => Err(rawr::EvalAltResult::ErrorArrayBounds(arr.len(), index, rawr::Position::NONE).into()),
    });

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut a = vec![1, 2, 3];
                    a[3] += 1;
                    a[3] + a[100]
                "
            )
            .unwrap(),
        143
    );
}
