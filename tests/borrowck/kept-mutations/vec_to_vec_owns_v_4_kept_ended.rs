fn main() {
    let mut v = vec![1, 2];
    let w = v.to_vec();
    let __k = __keep(&v);
    __see(__k);
    v.push(3);
    println!("{w:?} {v:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
