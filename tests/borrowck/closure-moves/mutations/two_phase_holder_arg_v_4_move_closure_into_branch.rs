fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    r.push(v.len());
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
