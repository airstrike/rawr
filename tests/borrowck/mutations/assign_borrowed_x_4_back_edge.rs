fn main() {
    let mut x = 1;
    let r = &x;
    let __b = &mut x;
    for __i in 0..2 {
        __touch(__b);
        x = 2;
    }
    println!("{}", *r + x);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
