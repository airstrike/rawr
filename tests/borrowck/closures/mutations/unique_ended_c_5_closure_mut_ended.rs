fn main() {
    let mut x = 1;
    let r = &mut x;
    let mut c = || *r += 1;
    let mut __f = || __touch(&mut c);
    __f();
    c();
    let s = &*r;
    println!("{}", s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
