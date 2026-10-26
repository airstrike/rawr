fn main() {
    let mut v = vec![1, 2];
    let w = v.to_vec();
    v.push(3);
    let __k = __keep(&v);
    for __i in 0..2 {
        __see(__k);
        println!("{w:?} {v:?}");
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
