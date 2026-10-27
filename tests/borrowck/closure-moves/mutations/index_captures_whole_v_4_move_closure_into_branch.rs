fn main() {
    let mut v = vec![1, 2];
    let mut c = || v[0] += 1;
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    println!("{}", v[1]);
    c();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
