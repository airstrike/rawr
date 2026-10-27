fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(v.len());
    if __c() {
        __take(v);
        panic!();
    }
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
