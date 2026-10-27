fn main() {
    let mut v = vec![1, 2, 3];
    for x in &mut v {
        let mut double = || *x *= 2;
        double();
    }
    let __b = &mut v;
    for __i in 0..2 {
        __touch(__b);
        println!("{v:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
