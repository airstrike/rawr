fn main() {
    let mut v = vec![1, 2];
    let it = v.iter();
    let __k = __keep(&v);
    for __i in 0..2 {
        __see(__k);
        v.push(3);
    }
    println!("{}", it.count());
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
