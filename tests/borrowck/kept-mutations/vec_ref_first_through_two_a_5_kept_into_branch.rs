fn main() {
    let mut a = 1;
    let refs = vec![&a];
    let f = refs.first();
    let __k = __keep(&a);
    a += 1;
    if __c() {
        __see(__k);
    }
    println!("{f:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
