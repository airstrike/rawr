fn main() {
    let mut x = 1;
    let r = &mut x;
    let n = match x {
        1 => 10,
        _ => 20,
    };
    *r += 1;
    let __b = &mut x;
    for __i in 0..2 {
        __touch(__b);
        println!("{} {}", n, x);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
