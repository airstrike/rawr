fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let c = || r.len();
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    v.push(1);
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
