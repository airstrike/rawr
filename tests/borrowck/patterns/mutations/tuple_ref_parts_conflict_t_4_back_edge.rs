fn main() {
    let mut t = (vec![1], vec![2]);
    let (ref mut a, _) = t;
    let __b = &mut t;
    for __i in 0..2 {
        __touch(__b);
        t.0.push(3);
    }
    a.push(4);
    println!("{:?}", t);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
