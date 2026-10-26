fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    let __v = vec![&a];
    refs.push(&a);
    __see(&__v);
    a += 1;
    println!("{refs:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
