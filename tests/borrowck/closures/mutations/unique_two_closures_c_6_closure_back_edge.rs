fn main() {
    let mut x = 1;
    let r = &mut x;
    let mut c = || *r += 1;
    let mut d = || *r += 2;
    let mut __f = || __touch(&mut c);
    for __i in 0..2 {
        __f();
        c();
    }
    d();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
