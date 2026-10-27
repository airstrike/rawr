fn main() {
    let mut v = vec![1, 2];
    let s = &v;
    let c = || &s[0];
    let r = c();
    v.push(3);
    let __m = move || __see(&v);
    __m();
    println!("{}", r);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
