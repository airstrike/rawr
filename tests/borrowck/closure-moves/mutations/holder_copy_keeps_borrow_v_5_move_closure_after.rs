fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    v.push(2);
    let __m = move || __see(&v);
    __m();
    println!("{}", s.len());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
