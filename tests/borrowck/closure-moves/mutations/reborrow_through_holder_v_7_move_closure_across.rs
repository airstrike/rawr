fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let s = &mut *r;
    s.push(2);
    r.push(3);
    let __m = move || __see(&v);
    println!("{:?}", v);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
