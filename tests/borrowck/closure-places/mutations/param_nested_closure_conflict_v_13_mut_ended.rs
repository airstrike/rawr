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
    bump(&mut v);
    let __b = &mut v;
    __touch(__b);
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
