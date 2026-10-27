fn call<F: FnMut()>(mut f: F) {
    f();
    f();
}
fn main() {
    let mut v = vec![1, 2];
    call(|| v.push(3));
    let __m = move || __see(&v);
    __m();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
