fn main() {
    let mut x = 1;
    let r = &x;
    let __h = || &x;
    let __k = __h();
    x = 2;
    __see(__k);
    println!("{}", *r + x);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
