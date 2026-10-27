fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    if __c() {
        __take(v);
        panic!();
    }
    v[0] = 5;
    println!("{} {:?}", first, v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
