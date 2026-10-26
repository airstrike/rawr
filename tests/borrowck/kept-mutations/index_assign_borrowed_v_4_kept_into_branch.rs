fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let __k = __keep(&v);
    v[0] = 5;
    if __c() {
        __see(__k);
    }
    println!("{}", r[0]);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
