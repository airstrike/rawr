fn main() {
    let mut v = vec![1];
    let mut a = || v.push(1);
    let __m = move || __see(&a);
    a();
    __m();
    let mut b = || v.push(2);
    b();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
