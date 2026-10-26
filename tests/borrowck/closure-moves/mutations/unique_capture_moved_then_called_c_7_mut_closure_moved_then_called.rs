fn main() {
    let mut n = 0;
    let r = &mut n;
    let mut c = || *r += 1;
    let mut d = c;
    d();
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
