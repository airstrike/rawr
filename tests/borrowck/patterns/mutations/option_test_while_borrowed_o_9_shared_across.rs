fn main() {
    let mut o = Some(1);
    let m = &mut o;
    let n = match o {
        Some(_) => 1,
        None => 2,
    };
    *m = None;
    let __s = &o;
    println!("{} {:?}", n, o);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
