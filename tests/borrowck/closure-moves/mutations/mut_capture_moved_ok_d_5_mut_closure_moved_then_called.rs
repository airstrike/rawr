fn main() {
    let mut n = 0;
    let c = || n += 1;
    let mut d = c;
    let mut __f = || __touch(&mut d);
    let mut __g = __f;
    __g();
    __f();
    d();
    d();
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
