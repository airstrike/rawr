fn main() {
    let mut x = 1;
    let r = &mut x;
    let n = match x {
        1 => 10,
        _ => 20,
    };
    *r += 1;
    let __b = &mut x;
    println!("{} {}", n, x);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
