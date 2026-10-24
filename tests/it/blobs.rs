#![cfg(not(feature = "no_index"))]
use rawr::{Blob, Dynamic, Engine, INT};
use std::iter::FromIterator;

/// A script's `Vec<u8>` comes back to the host as an array of integers.
fn bytes(value: Dynamic) -> Vec<INT> {
    value.into_typed_array::<INT>().unwrap()
}

#[test]
fn test_blobs() {
    let a = Blob::from_iter([1, 2, 3]);
    assert_eq!(a, [1, 2, 3]);

    let engine = Engine::new();
    let x = "let mut x: Vec<u8> = vec![1, 2, 3];";

    assert_eq!(engine.eval::<INT>(&format!("{x} x[1]")).unwrap(), 2);
    assert_eq!(engine.eval::<INT>(&format!("{x} x[0]")).unwrap(), 1);
    assert_eq!(engine.eval::<INT>(&format!("{x} x[x.len() - 1]")).unwrap(), 3);
    let _ = engine.eval::<INT>(&format!("{x} x[3]")).unwrap_err();

    #[cfg(not(feature = "no_object"))]
    {
        assert_eq!(bytes(engine.eval::<Dynamic>(&format!("{x} x.push(4); x")).unwrap()), [1, 2, 3, 4]);
        assert_eq!(bytes(engine.eval::<Dynamic>(&format!("{x} x.insert(0, 4); x")).unwrap()), [4, 1, 2, 3]);
        assert_eq!(bytes(engine.eval::<Dynamic>(&format!("{x} x.insert(3, 4); x")).unwrap()), [1, 2, 3, 4]);
        let _ = engine.eval::<Dynamic>(&format!("{x} x.insert(999, 4); x")).unwrap_err();
        assert_eq!(engine.eval::<INT>(&format!("{x} let z = vec![42]; x[z.len()]")).unwrap(), 2);
        assert_eq!(engine.eval::<INT>(&format!("{x} let z = vec![2]; x[z[0]]")).unwrap(), 3);
    }

    assert_eq!(bytes(engine.eval::<Dynamic>(&format!("{x} x.extend(x.clone()); x")).unwrap()), [1, 2, 3, 1, 2, 3]);
    assert_eq!(bytes(engine.eval::<Dynamic>(&format!("{x} x.iter().chain(x.iter()).copied().collect::<Vec<u8>>()")).unwrap()), [1, 2, 3, 1, 2, 3]);
    // `as u8` keeps the low byte, as in Rust.
    assert_eq!(bytes(engine.eval::<Dynamic>(&format!("{x} x.push(999_i64 as u8); x")).unwrap()), [1, 2, 3, 0xe7]);
    assert_eq!(bytes(engine.eval::<Dynamic>(&format!("{x} x[2] = 999_i64 as u8; x")).unwrap()), [1, 2, 0xe7]);
}

#[test]
fn test_blobs_write_string() {
    let engine = Engine::new();

    let expected = |text: &[u8]| text.iter().map(|&b| b as INT).collect::<Vec<_>>();

    assert_eq!(
        bytes(engine.eval::<Dynamic>(r#"let mut x = vec![0u8; 16]; x[0..13].copy_from_slice("hello, world!".as_bytes()); x"#).unwrap()),
        expected(b"hello, world!\0\0\0")
    );
    assert_eq!(
        bytes(engine.eval::<Dynamic>(r#"let mut x = vec![0u8; 10]; x[3..8].copy_from_slice(&"hello, world!".as_bytes()[..5]); x"#).unwrap()),
        expected(b"\0\0\0hello\0\0")
    );
    assert_eq!(
        bytes(engine.eval::<Dynamic>(r#"let mut x = vec![0u8; 10]; x[3..9].copy_from_slice("\u{2764}\u{2764}".as_bytes()); x"#).unwrap()),
        expected("\0\0\0\u{2764}\u{2764}\0".as_bytes())
    );
    assert_eq!(bytes(engine.eval::<Dynamic>(r#""hello".bytes().collect::<Vec<u8>>()"#).unwrap()), expected(b"hello"));
}
