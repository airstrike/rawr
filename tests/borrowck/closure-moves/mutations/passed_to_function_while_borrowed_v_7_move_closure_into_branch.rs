fn call<F: FnMut()>(mut f: F) {
    f();
}
fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    call(|| v.push(3));
    println!("{:?}", r);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
