fn call<F: FnMut()>(mut f: F) {
    f();
    f();
}
fn main() {
    let mut v = vec![1, 2];
    let __h = || &v;
    let __k = __h();
    call(|| v.push(3));
    __see(__k);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
