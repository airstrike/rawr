fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    x = 2;
    let __f = || __see(&x);
    let __g = __f;
    __f();
    println!("{}", x + y);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
