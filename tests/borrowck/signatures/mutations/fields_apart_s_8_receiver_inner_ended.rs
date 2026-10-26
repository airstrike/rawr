struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    let __t = vec![&s].to_vec();
    __see(&__t);
    s.b += 10;
    *r += 100;
    println!("{} {}", s.a, s.b);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
