fn main() {
    let mut v = vec![1, 2, 3];
    let mut fs = Vec::new();
    for x in v.iter_mut() {
        fs.push(move || *x += 100);
    }
    for mut f in fs {
        f();
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
