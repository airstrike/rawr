fn main() {
    let mut v = vec![1];
    let r = &v;
    let __k = __keep(&v);
    __see(__k);
    v.push(1);
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
