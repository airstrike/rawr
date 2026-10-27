fn main() {
    let mut x = 1;
    let mut y = 2;
    let mut r = &mut x;
    let mut c = || *r += 1;
    let s = &*r;
    c();
    r = &mut y;
    let mut __f = || __touch(&mut r);
    let mut __g = __f;
    println!("{} {}", s, r);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
