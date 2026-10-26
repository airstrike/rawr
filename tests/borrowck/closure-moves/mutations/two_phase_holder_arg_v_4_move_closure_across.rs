fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let __m = move || __see(&v);
    r.push(v.len());
    __m();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
