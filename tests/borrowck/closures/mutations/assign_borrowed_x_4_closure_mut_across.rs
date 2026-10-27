fn main() {
    let mut x = 1;
    let r = &x;
    let mut __f = || __touch(&mut x);
    x = 2;
    __f();
    println!("{}", *r + x);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
