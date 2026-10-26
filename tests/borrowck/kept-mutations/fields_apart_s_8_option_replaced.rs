struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    let mut __o = Some(&s);
    __see(&__o);
    __o = None;
    s.b += 10;
    __see(&__o);
    *r += 100;
    println!("{} {}", s.a, s.b);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
