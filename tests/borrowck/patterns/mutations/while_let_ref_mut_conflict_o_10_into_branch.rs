fn main() {
    let mut o = Some(3);
    while let Some(ref mut n) = o {
        let c = o.is_some();
        if *n == 0 || !c {
            break;
        }
        *n -= 1;
    }
    let __b = &mut o;
    println!("{:?}", o);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
