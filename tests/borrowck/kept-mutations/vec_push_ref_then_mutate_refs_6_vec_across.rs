fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    refs.push(&a);
    a += 1;
    let __v = vec![&refs];
    println!("{refs:?}");
    __see(&__v);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
