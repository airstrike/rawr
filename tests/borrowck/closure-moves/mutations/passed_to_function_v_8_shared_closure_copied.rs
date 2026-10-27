fn call<F: FnMut()>(mut f: F) {
    f();
    f();
}
fn main() {
    let mut v = vec![1, 2];
    call(|| v.push(3));
    let __f = || __see(&v);
    let __g = __f;
    __f();
    println!("{:?}", v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
