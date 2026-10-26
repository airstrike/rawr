fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    println!("{}", r.len());
    let __k = __keep(&r);
    for __i in 0..2 {
        __see(__k);
        r = &b;
    }
    a.push(3);
    println!("{} {:?}", r.len(), a);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
