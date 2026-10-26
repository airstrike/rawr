fn main() {
    let mut v = vec![1, 2, 3];
    let s: &mut [i64] = &mut v;
    if let [first, .., last] = s {
        *first += 10;
        *last += 20;
    }
    let __s = &v;
    println!("{:?}", v);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
