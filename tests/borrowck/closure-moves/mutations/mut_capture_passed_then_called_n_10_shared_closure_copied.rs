fn call(mut f: impl FnMut()) {
    f();
}

fn main() {
    let mut n = 0;
    let mut c = || n += 1;
    call(c);
    c();
    let __f = || __see(&n);
    let __g = __f;
    __f();
    println!("{}", n);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
