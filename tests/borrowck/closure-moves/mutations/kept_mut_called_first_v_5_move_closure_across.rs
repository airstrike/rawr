fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(1);
    c();
    let __m = move || __see(&v);
    v.len();
    __m();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
