fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    x = 2;
    let __b = &mut x;
    println!("{}", x + y);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
