fn main() {
    let mut x = 1;
    let mut y = 2;
    let mut r = &mut x;
    let mut c = || *r += 1;
    let s = &*r;
    c();
    let __g = || __see(&r);
    r = &mut y;
    __g();
    println!("{} {}", s, r);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
