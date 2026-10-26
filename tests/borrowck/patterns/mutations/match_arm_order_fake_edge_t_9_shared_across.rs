fn main() {
    let mut t = (false, 1);
    let r = &mut t.1;
    match t {
        (false, 1) => {}
        (true, _) => *r += 1,
        (false, _) => {}
    }
    let __s = &t;
    println!("{:?}", t);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
