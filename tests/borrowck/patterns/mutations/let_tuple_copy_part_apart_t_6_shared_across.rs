fn main() {
    let mut t = (1, 2);
    let r = &mut t.1;
    let (a, _) = t;
    *r += 1;
    let __s = &t;
    println!("{} {:?}", a, t);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
