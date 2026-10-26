fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    refs.push(&a);
    let __o = Some(&a);
    a += 1;
    __see(&__o);
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
