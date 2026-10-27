fn double(x: i32) -> i32 {
    x * 2
}

fn main() {
    let styles = vec![("double", double as fn(i32) -> i32)];
    let first: Vec<i32> = styles.into_iter().map(|(_, f)| f(3)).collect();
    let second: Vec<&str> = styles.into_iter().map(|(name, _)| name).collect();
    __take(second);
    println!("{first:?} {second:?}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
