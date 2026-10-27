fn call<F: FnMut()>(mut f: F) {
    f();
}
fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let __g = || __see(&v);
    call(|| v.push(3));
    __g();
    println!("{:?}", r);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
