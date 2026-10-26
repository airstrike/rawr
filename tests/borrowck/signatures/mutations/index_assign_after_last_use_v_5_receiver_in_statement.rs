fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    __see(&vec![&v].first());
    v[0] = 5;
    println!("{} {:?}", first, v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
