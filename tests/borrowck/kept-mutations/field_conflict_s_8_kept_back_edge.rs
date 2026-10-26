struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    let __k = __keep(&s);
    for __i in 0..2 {
        __see(__k);
        s.a += 10;
    }
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
