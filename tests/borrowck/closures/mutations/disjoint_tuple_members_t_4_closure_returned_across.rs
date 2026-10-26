fn main() {
    let mut t = (vec![1], vec![2]);
    let mut c = || t.0.push(3);
    let __h = || &t;
    let __k = __h();
    t.1.push(4);
    __see(__k);
    c();
    println!("{:?} {:?}", t.0, t.1);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
