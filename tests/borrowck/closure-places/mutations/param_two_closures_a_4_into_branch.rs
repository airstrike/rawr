fn bump(r: &mut Vec<i32>) {
    let mut a = || r.push(1);
    let mut b = || r.push(2);
    let __b = &mut a;
    a();
    if __c() {
        __touch(__b);
    }
    b();
}

fn main() {
    let mut v = vec![0];
    bump(&mut v);
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
