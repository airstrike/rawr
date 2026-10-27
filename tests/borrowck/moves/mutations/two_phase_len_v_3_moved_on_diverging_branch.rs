fn main() {
    let mut v = vec![1, 2];
    if __c() {
        __take(v);
        panic!();
    }
    v.push(v.len());
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
