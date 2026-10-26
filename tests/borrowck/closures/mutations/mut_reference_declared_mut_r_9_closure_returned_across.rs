fn main() {
    let mut x = 1;
    let mut y = 2;
    let mut r = &mut x;
    let mut c = || *r += 1;
    let s = &*r;
    c();
    r = &mut y;
    let __h = || &r;
    let __k = __h();
    println!("{} {}", s, r);
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
