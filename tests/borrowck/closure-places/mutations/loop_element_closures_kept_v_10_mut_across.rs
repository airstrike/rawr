fn main() {
    let mut v = vec![1, 2, 3];
    let mut fs = Vec::new();
    for x in v.iter_mut() {
        fs.push(move || *x += 100);
    }
    for mut f in fs {
        f();
    }
    let __b = &mut v;
    println!("{v:?}");
    __touch(__b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
