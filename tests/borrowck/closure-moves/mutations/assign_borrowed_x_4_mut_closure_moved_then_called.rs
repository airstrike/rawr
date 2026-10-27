fn main() {
    let mut x = 1;
    let r = &x;
    let mut __f = || __touch(&mut x);
    let mut __g = __f;
    __g();
    __f();
    x = 2;
    println!("{}", *r + x);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
