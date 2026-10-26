fn main() {
    let mut v = vec![1];
    let ref mut a = v;
    a.push(2);
    let __b = &mut v;
    for __i in 0..2 {
        __touch(__b);
        v.push(1);
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
