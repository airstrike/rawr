fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let __m = move || __see(&v);
    v.push(r.len());
    __m();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
