fn main() {
    let mut t = (1, 2);
    let r = &mut t.0;
    let (a, _) = t;
    *r += 1;
    let __b = &mut t;
    for __i in 0..2 {
        __touch(__b);
        println!("{} {:?}", a, t);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
