fn bump(r: &mut Vec<i32>) {
    let mut push = |x| r.push(x);
    let __b = &mut push;
    push(1);
    if __c() {
        __touch(__b);
    }
    push(2);
}

fn main() {
    let mut v = vec![0];
    bump(&mut v);
    bump(&mut v);
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
