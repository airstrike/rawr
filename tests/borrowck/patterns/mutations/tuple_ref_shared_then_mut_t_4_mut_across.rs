fn main() {
    let mut t = (vec![1], vec![2]);
    let (ref a, _) = t;
    let __b = &mut t;
    t.0.push(1);
    __touch(__b);
    println!("{:?}", a);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
