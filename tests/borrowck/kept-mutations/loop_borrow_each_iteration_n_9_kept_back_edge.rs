fn main() {
    let mut v = vec![1];
    let mut n = 0;
    while n < 3 {
        let r = &mut v;
        r.push(n);
        n += v.len() as i64;
    }
    let __k = __keep(&n);
    for __i in 0..2 {
        __see(__k);
        println!("{n} {:?}", v);
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
