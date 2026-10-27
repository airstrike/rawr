fn bump(r: &mut Vec<i32>) {
    let mut outer = || {
        let mut inner = |x| r.push(x);
        let __s = &inner;
        inner(1);
        __see(__s);
        inner(2);
    };
    outer();
    outer();
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
