fn main() {
    let mut v = vec![vec![1], vec![2]];
    let r = &mut v[1];
    let mut f = |x| r.push(x);
    f(3);
    f(4);
    for __i in 0..2 {
        let __b = &mut v;
        __touch(__b);
        println!("{v:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
