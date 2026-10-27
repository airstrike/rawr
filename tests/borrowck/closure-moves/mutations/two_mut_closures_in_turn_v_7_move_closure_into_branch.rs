fn main() {
    let mut v = vec![1];
    let mut a = || v.push(1);
    a();
    let mut b = || v.push(2);
    b();
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
