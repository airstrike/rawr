fn main() {
    let mut v = vec![1, 2];
    let s: &mut [i64] = &mut v;
    let w = &mut *s;
    let n = match s {
        [] => 0,
        _ => 1,
    };
    w[0] += 1;
    let __s = &v;
    println!("{} {:?}", n, v);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
