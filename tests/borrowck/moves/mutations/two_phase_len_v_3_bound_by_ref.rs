fn main() {
    let mut v = vec![1, 2];
    let ref __v = v;
    v.push(v.len());
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
