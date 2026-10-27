fn main() {
    let mut v = vec![1, 2, 3];
    for x in &mut v {
        let mut double = || *x *= 2;
        double();
    }
    let __s = &v;
    println!("{v:?}");
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
