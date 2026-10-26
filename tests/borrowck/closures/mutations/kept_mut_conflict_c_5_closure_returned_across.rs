fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(1);
    v.len();
    let __h = || &c;
    let __k = __h();
    c();
    __see(__k);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
