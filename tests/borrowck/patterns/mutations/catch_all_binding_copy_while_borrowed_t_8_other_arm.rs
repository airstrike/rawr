fn main() {
    let mut t = (1, 2);
    let m = &mut t;
    let n = match t {
        x => x.0,
    };
    m.0 += 1;
    let __b = &mut t;
    if __c() {
        __touch(__b);
    } else {
        println!("{} {:?}", n, t);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
