fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    refs.push(&a);
    println!("{refs:?}");
    let __h = __Hold(&a);
    a += 1;
    __see(&__h);
    println!("{a}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
