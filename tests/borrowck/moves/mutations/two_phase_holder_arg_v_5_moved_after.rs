fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(v.len());
    println!("{:?}", v);
    __take(v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
