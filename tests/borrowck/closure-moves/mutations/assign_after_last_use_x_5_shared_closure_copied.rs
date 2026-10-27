fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    let __f = || __see(&x);
    let __g = __f;
    __f();
    x = 2;
    __g();
    println!("{}", x + y);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
