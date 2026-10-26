fn main() {
    let mut x = 1;
    let r = &mut x;
    let mut c = || *r += 1;
    let s = &*r;
    let mut __f = || __touch(&mut c);
    let mut __g = __f;
    c();
    __g();
    println!("{}", s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
