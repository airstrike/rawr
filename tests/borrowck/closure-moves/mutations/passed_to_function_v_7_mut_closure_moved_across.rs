fn call<F: FnMut()>(mut f: F) {
    f();
    f();
}
fn main() {
    let mut v = vec![1, 2];
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    call(|| v.push(3));
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
