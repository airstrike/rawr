fn main() {
    let mut o = Some(3);
    while let Some(ref mut n) = o {
        if *n == 0 {
            o = None;
        } else {
            *n -= 1;
        }
    }
    let __s = &o;
    println!("{:?}", o);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
