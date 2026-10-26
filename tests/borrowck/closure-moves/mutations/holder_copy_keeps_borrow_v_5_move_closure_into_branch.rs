fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    v.push(2);
    println!("{}", s.len());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
