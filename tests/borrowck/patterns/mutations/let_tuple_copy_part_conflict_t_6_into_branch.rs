fn main() {
    let mut t = (1, 2);
    let r = &mut t.0;
    let (a, _) = t;
    *r += 1;
    let __b = &mut t;
    println!("{} {:?}", a, t);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
