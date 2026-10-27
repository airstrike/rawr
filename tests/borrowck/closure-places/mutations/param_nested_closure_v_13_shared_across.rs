fn bump(r: &mut Vec<i32>) {
    let mut outer = || {
        let mut inner = |x| r.push(x);
        inner(1);
        inner(2);
    };
    outer();
    outer();
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
