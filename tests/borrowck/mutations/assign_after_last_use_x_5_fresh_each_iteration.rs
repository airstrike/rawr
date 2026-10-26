fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    for __i in 0..2 {
        let __b = &mut x;
        __touch(__b);
        x = 2;
    }
    println!("{}", x + y);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
