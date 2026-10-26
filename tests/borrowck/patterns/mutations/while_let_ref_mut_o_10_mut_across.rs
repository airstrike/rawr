fn main() {
    let mut o = Some(3);
    while let Some(ref mut n) = o {
        if *n == 0 {
            o = None;
        } else {
            *n -= 1;
        }
    }
    let __b = &mut o;
    println!("{:?}", o);
    __touch(__b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
