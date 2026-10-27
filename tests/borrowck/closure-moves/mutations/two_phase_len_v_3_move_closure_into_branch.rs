fn main() {
    let mut v = vec![1, 2];
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    v.push(v.len());
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
