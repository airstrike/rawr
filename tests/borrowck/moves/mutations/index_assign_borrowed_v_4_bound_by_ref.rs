fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let ref __v = v;
    v[0] = 5;
    println!("{}", r[0]);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
