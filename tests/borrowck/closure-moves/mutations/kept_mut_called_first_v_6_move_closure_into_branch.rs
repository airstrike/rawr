fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(1);
    c();
    v.len();
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
