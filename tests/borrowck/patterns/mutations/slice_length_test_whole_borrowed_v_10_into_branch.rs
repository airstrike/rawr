fn main() {
    let mut v = vec![1, 2];
    let s: &mut [i64] = &mut v;
    let w = &mut *s;
    let n = match s {
        [] => 0,
        _ => 1,
    };
    w[0] += 1;
    let __b = &mut v;
    println!("{} {:?}", n, v);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
