fn main() {
    let mut v = vec![1, 2];
    let __b = &mut v;
    for __i in 0..2 {
        __touch(__b);
        v.push(v.len());
    }
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
