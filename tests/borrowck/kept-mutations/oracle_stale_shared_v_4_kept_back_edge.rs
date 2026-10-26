fn main() {
    let mut v = vec![1];
    let r = &v;
    let __k = __keep(&v);
    for __i in 0..2 {
        __see(__k);
        v.push(1);
    }
    println!("{r:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
