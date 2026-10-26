fn main() {
    let mut t = (vec![1], vec![2]);
    let (ref mut a, _) = t;
    let __b = &mut t;
    t.1.push(3);
    if __c() {
        __touch(__b);
    }
    a.push(4);
    println!("{:?}", t);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
