fn main() {
    let mut o = Some(1);
    let m = &mut o;
    let n = match o {
        Some(_) => 1,
        None => 2,
    };
    *m = None;
    let __b = &mut o;
    println!("{} {:?}", n, o);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
