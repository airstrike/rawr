fn first(v: &Vec<i32>) -> &i32 {
    &v[0]
}

fn main() {
    let mut v = vec![1, 2, 3];
    let f = first(&v);
    println!("{f}");
    let __k = __keep(&v);
    for __i in 0..2 {
        __see(__k);
        v.push(4);
    }
    println!("{v:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
