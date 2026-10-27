fn main() {
    let mut x = 1;
    let r = &mut x;
    let s = &*r;
    let mut c = || *r += 1;
    let __g = || __see(&c);
    c();
    __g();
    println!("{}", s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
