fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let g = || r.len();
    let mut f = || r.push(2);
    for __i in 0..2 {
        let __b = &mut f;
        __touch(__b);
        f();
    }
    println!("{}", g());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
