fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let __b = &mut v;
    if __c() {
        __touch(__b);
    } else {
        v.push(r.len());
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
