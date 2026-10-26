fn main() {
    let mut v = vec![1];
    let mut a = || v.push(1);
    let mut b = || v.push(2);
    let __g = || __see(&a);
    a();
    __g();
    b();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
