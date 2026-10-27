fn main() {
    let mut v = vec![1, 2];
    let mut c = || v[0] += 1;
    println!("{}", v[1]);
    let __m = move || __see(&c);
    c();
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
