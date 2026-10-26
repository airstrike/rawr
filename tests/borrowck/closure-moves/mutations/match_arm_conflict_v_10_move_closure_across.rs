fn main() {
    let mut v = vec![1, 2];
    let k = v.len();
    let r = &mut v;
    match k {
        1 => v.push(1),
        _ => r.push(2),
    }
    r.push(3);
    let __m = move || __see(&v);
    println!("{:?}", v);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
