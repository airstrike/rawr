fn main() {
    let mut x = 1;
    let r = &x;
    let __g = || __see(&x);
    x = 2;
    __g();
    println!("{}", *r + x);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
