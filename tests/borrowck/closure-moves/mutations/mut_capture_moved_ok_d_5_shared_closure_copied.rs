fn main() {
    let mut n = 0;
    let c = || n += 1;
    let mut d = c;
    let __f = || __see(&d);
    let __g = __f;
    __f();
    d();
    __g();
    d();
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
