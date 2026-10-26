fn main() {
    let mut o = Some(3);
    while let Some(ref mut n) = o {
        if *n == 0 {
            o = None;
        } else {
            *n -= 1;
        }
    }
    for __i in 0..2 {
        let __b = &mut o;
        __touch(__b);
        println!("{:?}", o);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
