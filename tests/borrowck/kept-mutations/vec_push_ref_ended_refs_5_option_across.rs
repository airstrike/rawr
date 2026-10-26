fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    refs.push(&a);
    let __o = Some(&refs);
    println!("{refs:?}");
    __see(&__o);
    a += 1;
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
