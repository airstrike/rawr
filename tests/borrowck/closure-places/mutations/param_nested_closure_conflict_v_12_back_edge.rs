fn bump(r: &mut Vec<i32>) {
    let mut outer = || {
        let mut inner = |x| r.push(x);
        r.push(9);
        inner(1);
    };
    outer();
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
