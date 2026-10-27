struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    let __d = __See(&s);
    if __c() {
        drop(__d);
    }
    s.a += 10;
    *r += 100;
    println!("{} {}", s.a, s.b);
}

fn __c() -> bool {
    false
}
fn __eat<T>(_: T) {}
struct __See<'a, T>(&'a T);
impl<T> Drop for __See<'_, T> {
    fn drop(&mut self) {}
}
struct __Hold<'a, T> {
    s: __See<'a, T>,
}
