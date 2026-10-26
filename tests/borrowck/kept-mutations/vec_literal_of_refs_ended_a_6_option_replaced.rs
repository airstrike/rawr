fn main() {
    let mut a = 1;
    let b = 2;
    let refs = vec![&a, &b];
    println!("{refs:?}");
    let mut __o = Some(&a);
    __see(&__o);
    __o = None;
    a = 5;
    __see(&__o);
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
