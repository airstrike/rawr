fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    v[0] = 5;
    println!("{} {:?}", first, v);
    __take(v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
