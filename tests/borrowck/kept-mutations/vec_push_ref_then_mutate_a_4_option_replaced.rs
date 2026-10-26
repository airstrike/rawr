fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    let mut __o = Some(&a);
    __see(&__o);
    __o = None;
    refs.push(&a);
    __see(&__o);
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
