fn main() {
    let mut x = 5;
    let r = &mut x;
    let y = x;
    *r += 1;
    for __i in 0..2 {
        let __b = &mut x;
        __touch(__b);
        println!("{} {}", x, y);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
