fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    x = 2;
    let __h = || &x;
    let __k = __h();
    println!("{}", x + y);
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
