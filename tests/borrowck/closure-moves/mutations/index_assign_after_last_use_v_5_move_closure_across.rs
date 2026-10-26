fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    let __m = move || __see(&v);
    v[0] = 5;
    __m();
    println!("{} {:?}", first, v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
