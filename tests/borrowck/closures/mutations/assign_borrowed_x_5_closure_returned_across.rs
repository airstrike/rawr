fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let __h = || &x;
    let __k = __h();
    println!("{}", *r + x);
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
