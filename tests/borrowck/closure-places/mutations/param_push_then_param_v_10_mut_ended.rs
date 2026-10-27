fn bump(r: &mut Vec<i32>) {
    let mut push = |x| r.push(x);
    push(1);
    push(2);
    r.push(3);
}

fn main() {
    let mut v = vec![0];
    let __b = &mut v;
    __touch(__b);
    bump(&mut v);
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
