fn main() {
    let mut x = 1;
    let mut y = 2;
    let mut r = &mut x;
    let mut c = || *r += 1;
    let s = &*r;
    c();
    let mut __f = || __touch(&mut y);
    for __i in 0..2 {
        __f();
        r = &mut y;
    }
    println!("{} {}", s, r);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
