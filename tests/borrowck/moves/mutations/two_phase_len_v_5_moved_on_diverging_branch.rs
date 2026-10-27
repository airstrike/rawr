fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    if __c() {
        __take(v);
        panic!();
    }
    v.push(r.len());
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
