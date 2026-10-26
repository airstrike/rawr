fn main() {
    let mut v = vec![1, 2];
    v.push({
        v.clear();
        3
    });
    let __k = __keep(&v);
    println!("{:?}", v);
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
