struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    s.a += 10;
    *r += 100;
    let __f = || __see(&s);
    let __g = __f;
    __f();
    println!("{} {}", s.a, s.b);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
