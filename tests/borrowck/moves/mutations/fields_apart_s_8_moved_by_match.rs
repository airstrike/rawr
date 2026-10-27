struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    match s {
        __v => __take(__v),
    }
    s.b += 10;
    *r += 100;
    println!("{} {}", s.a, s.b);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
