fn bump(r: &mut Vec<i32>) {
    let mut a = || r.push(1);
    a();
    let mut b = || r.push(2);
    for __i in 0..2 {
        let __b = &mut b;
        __touch(__b);
        b();
    }
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
