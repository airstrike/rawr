fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let ref __v = v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
