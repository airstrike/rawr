fn main() {
    let mut x = 1;
    let r = &x;
    let __b = &mut x;
    x = 2;
    __touch(__b);
    println!("{}", *r + x);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
