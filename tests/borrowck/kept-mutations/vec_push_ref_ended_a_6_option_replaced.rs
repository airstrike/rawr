fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    refs.push(&a);
    println!("{refs:?}");
    let mut __o = Some(&a);
    __see(&__o);
    __o = None;
    a += 1;
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
