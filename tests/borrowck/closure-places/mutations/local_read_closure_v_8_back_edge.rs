fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(3);
    let len = || r.len();
    println!("{}", len());
    r.push(4);
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
