fn double(x: i32) -> i32 {
    x * 2
}

fn negate(x: i32) -> i32 {
    -x
}

fn main() {
    let styles = [("double", double as fn(i32) -> i32), ("negate", negate)];
    let first: Vec<i32> = styles.into_iter().map(|(_, f)| f(3)).collect();
    let second: Vec<&str> = styles.into_iter().map(|(name, _)| name).collect();
    for __i in 0..2 {
        if __c() {
            __take(first);
        }
    }
    println!("{first:?} {second:?}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
