fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    v[0] = 5;
    let __m = move || __see(&v);
    __m();
    println!("{}", r[0]);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
