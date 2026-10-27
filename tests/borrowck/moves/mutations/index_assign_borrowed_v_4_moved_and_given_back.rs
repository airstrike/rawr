fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let __m = v;
    v = __m;
    v[0] = 5;
    println!("{}", r[0]);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
