fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    v[0] = 5;
    let __m = v;
    v = __m;
    println!("{} {:?}", first, v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
