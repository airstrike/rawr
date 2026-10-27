fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(1);
    let __m = move || __see(&v);
    v.len();
    __m();
    c();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
