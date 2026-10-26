fn main() {
    let mut v = vec![1];
    let mut a = || v.push(1);
    let mut b = || v.push(2);
    let __h = || &a;
    let __k = __h();
    a();
    __see(__k);
    b();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
