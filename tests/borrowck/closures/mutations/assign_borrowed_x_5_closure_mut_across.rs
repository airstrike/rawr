fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let mut __f = || __touch(&mut x);
    println!("{}", *r + x);
    __f();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
