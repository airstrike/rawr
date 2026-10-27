fn main() {
    let mut n = 1;
    let mut c = || n += 1;
    let __f = || __see(&c);
    let __g = __f;
    __f();
    c();
    __g();
    c();
    let m = n;
    println!("{}", m);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
