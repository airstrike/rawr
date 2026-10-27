fn main() {
    let mut t = (vec![1], vec![2]);
    let mut c = || t.0.push(3);
    t.1.push(4);
    c();
    let __m = move || __see(&t);
    println!("{:?} {:?}", t.0, t.1);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
