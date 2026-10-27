fn main() {
    let mut v = vec![1, 2];
    let w = vec![3];
    let s = &w;
    let c = || s;
    let r = c();
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    v.push(3);
    println!("{:?} {:?}", r, v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
