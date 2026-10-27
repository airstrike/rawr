fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    v[0] = 5;
    __take(v);
    println!("{}", r[0]);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
