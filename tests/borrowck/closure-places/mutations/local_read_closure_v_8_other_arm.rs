fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(3);
    let len = || r.len();
    println!("{}", len());
    r.push(4);
    let __b = &mut v;
    if __c() {
        __touch(__b);
    } else {
        println!("{v:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
