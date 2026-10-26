fn main() {
    let mut v = vec![1, 2];
    let __k = __keep(&v);
    for __i in 0..2 {
        __see(__k);
        v.push(v.len());
    }
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
