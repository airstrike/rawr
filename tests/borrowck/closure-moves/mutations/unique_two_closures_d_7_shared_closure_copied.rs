fn main() {
    let mut x = 1;
    let r = &mut x;
    let mut c = || *r += 1;
    let mut d = || *r += 2;
    c();
    let __f = || __see(&d);
    let __g = __f;
    __f();
    d();
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
