fn bump(r: &mut Vec<i32>) {
    let first = &r[0];
    let mut push = |x| r.push(x);
    push(1);
    println!("{first}");
}

fn main() {
    let mut v = vec![0];
    bump(&mut v);
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
