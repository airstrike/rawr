fn main() {
    let mut v = vec![1, 2];
    let n = v.len();
    v.push(3);
    let __k = __keep(&v);
    println!("{n} {v:?}");
    if __c() {
        __see(__k);
    }
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
