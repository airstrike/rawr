fn call(mut f: impl FnMut()) {
    f();
}

fn main() {
    let mut n = 0;
    let mut c = || n += 1;
    call(c);
    let mut __f = || __touch(&mut c);
    let mut __g = __f;
    __g();
    __f();
    c();
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
