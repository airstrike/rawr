fn main() {
    let mut v = vec![1, 2, 3];
    let mut fs = Vec::new();
    for x in v.iter_mut() {
        fs.push(move || *x += 100);
    }
    for mut f in fs {
        f();
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
