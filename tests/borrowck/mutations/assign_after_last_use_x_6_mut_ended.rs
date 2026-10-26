fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    x = 2;
    let __b = &mut x;
    __touch(__b);
    println!("{}", x + y);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
