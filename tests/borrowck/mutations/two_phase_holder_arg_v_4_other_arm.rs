fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let __b = &mut v;
    if __c() {
        __touch(__b);
    } else {
        r.push(v.len());
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
