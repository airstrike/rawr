struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    let __t = vec![&s].to_vec();
    s.a += 10;
    __see(&__t);
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
