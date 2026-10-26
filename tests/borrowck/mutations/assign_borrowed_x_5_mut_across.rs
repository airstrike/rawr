fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let __b = &mut x;
    println!("{}", *r + x);
    __touch(__b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
