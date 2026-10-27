fn bump(r: &mut Vec<i32>) {
    let mut a = || r.push(1);
    a();
    let mut b = || r.push(2);
    b();
}

fn main() {
    let mut v = vec![0];
    let __b = &mut v;
    for __i in 0..2 {
        __touch(__b);
        bump(&mut v);
    }
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
