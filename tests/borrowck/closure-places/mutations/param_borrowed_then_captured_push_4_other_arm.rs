fn bump(r: &mut Vec<i32>) {
    let first = &r[0];
    let mut push = |x| r.push(x);
    let __b = &mut push;
    if __c() {
        __touch(__b);
    } else {
        push(1);
    }
    println!("{first}");
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
