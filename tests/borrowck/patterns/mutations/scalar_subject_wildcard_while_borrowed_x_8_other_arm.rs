fn main() {
    let mut x = 1;
    let r = &mut x;
    let n = match x {
        _ => 20,
    };
    *r += 1;
    let __b = &mut x;
    if __c() {
        __touch(__b);
    } else {
        println!("{} {}", n, x);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
