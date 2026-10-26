fn main() {
    let mut x = 1;
    let mut y = 2;
    let mut r = &mut x;
    let mut c = || *r += 1;
    let s = &*r;
    c();
    let __h = || &r;
    let __k = __h();
    r = &mut y;
    __see(__k);
    println!("{} {}", s, r);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
