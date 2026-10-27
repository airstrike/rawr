fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let ref __v = v;
    r.push(v.len());
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
