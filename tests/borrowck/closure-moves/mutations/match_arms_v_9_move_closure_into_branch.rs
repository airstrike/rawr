fn main() {
    let mut v = vec![1, 2];
    let k = v.len();
    let r = &mut v;
    match k {
        1 => v.push(1),
        _ => r.push(2),
    }
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
