fn main() {
    let mut v = vec![1];
    let mut a = || v.push(1);
    a();
    let mut b = || v.push(2);
    b();
    let __m = move || __see(&b);
    __m();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
