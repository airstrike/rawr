fn main() {
    let mut t = (1, 2);
    let r = &mut t.0;
    let n = match t {
        (1, _) => 10,
        _ => 20,
    };
    *r += 1;
    let __s = &t;
    println!("{} {:?}", n, t);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
