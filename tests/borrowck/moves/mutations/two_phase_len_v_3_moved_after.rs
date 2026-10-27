fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    __take(v);
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
