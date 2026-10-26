fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    for __i in 0..2 {
        let __b = &mut v;
        __touch(__b);
        r.push(v.len());
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
