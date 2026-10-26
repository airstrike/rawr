fn main() {
    let mut t = (vec![1], vec![2]);
    let mut c = || t.0.push(3);
    t.1.push(4);
    let __h = || &c;
    let __k = __h();
    c();
    __see(__k);
    println!("{:?} {:?}", t.0, t.1);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
