fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    v.push(r.len());
    __take(v);
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
