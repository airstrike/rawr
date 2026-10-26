fn main() {
    let mut t = (vec![1], vec![2]);
    let (ref mut a, _) = t;
    t.1.push(3);
    a.push(4);
    let __b = &mut t;
    println!("{:?}", t);
    __touch(__b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
