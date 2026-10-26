fn main() {
    let mut a = 1;
    let o = Some(&a);
    println!("{o:?}");
    a += 1;
    let __k = __keep(&a);
    println!("{a}");
    __see(__k);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
