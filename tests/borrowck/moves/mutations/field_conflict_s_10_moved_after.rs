struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    s.a += 10;
    *r += 100;
    println!("{} {}", s.a, s.b);
    __take(s);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
