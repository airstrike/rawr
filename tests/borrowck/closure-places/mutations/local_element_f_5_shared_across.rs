fn main() {
    let mut v = vec![vec![1], vec![2]];
    let r = &mut v[1];
    let mut f = |x| r.push(x);
    let __s = &f;
    f(3);
    __see(__s);
    f(4);
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
