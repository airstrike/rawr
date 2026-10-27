fn main() {
    let mut v = vec![1, 2, 3];
    for x in &mut v {
        let mut double = || *x *= 2;
        let __s = &double;
        double();
        __see(__s);
    }
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
