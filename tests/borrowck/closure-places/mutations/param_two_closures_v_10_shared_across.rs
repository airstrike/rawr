fn bump(r: &mut Vec<i32>) {
    let mut a = || r.push(1);
    let mut b = || r.push(2);
    a();
    b();
}

fn main() {
    let mut v = vec![0];
    let __s = &v;
    bump(&mut v);
    __see(__s);
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
