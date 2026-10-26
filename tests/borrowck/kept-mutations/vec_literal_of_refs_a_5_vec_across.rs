fn main() {
    let mut a = 1;
    let b = 2;
    let refs = vec![&a, &b];
    let __v = vec![&a];
    a = 5;
    __see(&__v);
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
