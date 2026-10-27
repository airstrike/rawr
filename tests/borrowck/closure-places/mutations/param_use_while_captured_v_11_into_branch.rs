fn bump(r: &mut Vec<i32>) {
    let mut push = |x| r.push(x);
    push(1);
    r.push(3);
    push(2);
}

fn main() {
    let mut v = vec![0];
    bump(&mut v);
    let __b = &mut v;
    println!("{v:?}");
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
