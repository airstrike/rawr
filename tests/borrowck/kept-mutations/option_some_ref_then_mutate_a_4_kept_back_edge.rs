fn main() {
    let mut a = 1;
    let o = Some(&a);
    let __k = __keep(&a);
    for __i in 0..2 {
        __see(__k);
        a += 1;
    }
    println!("{o:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
