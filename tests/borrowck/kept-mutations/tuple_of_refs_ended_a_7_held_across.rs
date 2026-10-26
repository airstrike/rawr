fn main() {
    let mut a = 1;
    let b = 2;
    let pair = (&a, &b);
    println!("{} {}", pair.0, pair.1);
    a += 1;
    let __h = __Hold(&a);
    println!("{a}");
    __see(&__h);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
