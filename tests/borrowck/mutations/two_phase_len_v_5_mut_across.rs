fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let __b = &mut v;
    v.push(r.len());
    __touch(__b);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
