fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    let __k = __keep(&refs);
    for __i in 0..2 {
        __see(__k);
        refs.push(&a);
    }
    println!("{refs:?}");
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
