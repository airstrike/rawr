fn main() {
    let mut t = (vec![1], vec![2]);
    let (ref mut a, _) = t;
    t.0.push(3);
    a.push(4);
    let __s = &t;
    println!("{:?}", t);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
