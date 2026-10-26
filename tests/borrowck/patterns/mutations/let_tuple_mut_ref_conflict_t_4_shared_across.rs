fn main() {
    let mut t = (vec![1], vec![2]);
    let (a, _) = &mut t;
    let __s = &t;
    t.1.push(3);
    __see(__s);
    a.push(4);
    println!("{:?}", t);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
