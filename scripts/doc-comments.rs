//! This script illustrates how to put doc-comments on functions.

/// The function `foo`, which prints `hello, world!` and a magic number,
/// accepts three parameters.
///
/// # Parameters
///
/// * `x` - `i64`
/// * `y` - `&str`
/// * `z` - `bool`
///
/// # Notes
///
/// This is a doc-comment.  It can be obtained with the `metadata` feature.
///
/// An example is the `rawr-doc` app.
///
/// # Example
///
/// ```rust
/// foo(42, "hello", true);     // prints 47
/// ```
fn foo(x: i64, y: &str, z: bool) {
    let n = if z { x + y.len() as i64 } else { x };
    println!("hello, world! {n}");
}

foo(39, "bar", true);
