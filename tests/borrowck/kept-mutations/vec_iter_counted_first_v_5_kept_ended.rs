fn main() {
    let mut v = vec![1, 2];
    let n = v.iter().count();
    v.push(3);
    let __k = __keep(&v);
    __see(__k);
    println!("{n} {v:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
