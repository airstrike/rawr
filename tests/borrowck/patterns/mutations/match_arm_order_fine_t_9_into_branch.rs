fn main() {
    let mut t = (false, 1);
    let r = &mut t.1;
    match t {
        (true, _) => *r += 1,
        (false, 1) => {}
        (false, _) => {}
    }
    let __b = &mut t;
    println!("{:?}", t);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
