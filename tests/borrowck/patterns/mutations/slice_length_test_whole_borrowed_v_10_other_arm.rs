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
    if __c() {
        __touch(__b);
    } else {
        println!("{} {:?}", n, v);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
