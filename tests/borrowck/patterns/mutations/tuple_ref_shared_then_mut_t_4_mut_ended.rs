fn main() {
    let mut t = (vec![1], vec![2]);
    let (ref a, _) = t;
    let __b = &mut t;
    __touch(__b);
    t.0.push(1);
    println!("{:?}", a);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
