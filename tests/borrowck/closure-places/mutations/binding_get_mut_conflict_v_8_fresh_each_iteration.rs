fn main() {
    let mut v = vec![1, 2, 3];
    if let Some(x) = v.get_mut(1) {
        let mut inc = || *x += 10;
        *x += 1;
        inc();
    }
    for __i in 0..2 {
        let __b = &mut v;
        __touch(__b);
        println!("{v:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
