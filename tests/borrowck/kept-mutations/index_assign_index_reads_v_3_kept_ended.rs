fn main() {
    let mut v = vec![1, 2];
    let __k = __keep(&v);
    __see(__k);
    v[v.len() - 1] = 5;
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
