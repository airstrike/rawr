fn main() {
    let mut v = vec![1, 2];
    let c = || &v;
    let r = c();
    v.push(3);
    let __m = move || __see(&v);
    __m();
    println!("{:?}", r);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
