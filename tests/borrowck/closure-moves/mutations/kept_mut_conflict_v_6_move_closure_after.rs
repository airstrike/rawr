fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(1);
    v.len();
    c();
    println!("{:?}", v);
    let __m = move || __see(&v);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
