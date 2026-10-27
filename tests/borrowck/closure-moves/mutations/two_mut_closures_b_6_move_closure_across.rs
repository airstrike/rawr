fn main() {
    let mut v = vec![1];
    let mut a = || v.push(1);
    let mut b = || v.push(2);
    a();
    let __m = move || __see(&b);
    b();
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
