fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(v.len());
    let ref __v = v;
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
