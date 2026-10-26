fn main() {
    let mut t = (1, 2);
    let r = &mut t.1;
    let n = match t {
        (1, _) => 10,
        _ => 20,
    };
    *r += 1;
    let __b = &mut t;
    println!("{} {:?}", n, t);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
