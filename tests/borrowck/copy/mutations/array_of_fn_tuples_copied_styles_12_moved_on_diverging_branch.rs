fn double(x: i32) -> i32 {
    x * 2
}

fn negate(x: i32) -> i32 {
    -x
}

fn main() {
    let styles = [("double", double as fn(i32) -> i32), ("negate", negate)];
    let copy = styles;
    if __c() {
        __take(styles);
        panic!();
    }
    println!("{} {}", (copy[0].1)(4), styles[1].0);
    println!("{} {}", (styles[1].1)(4), copy.len());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
