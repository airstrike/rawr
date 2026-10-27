fn main() {
    let mut v = vec![1];
    let r = &v;
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    v.push(1);
    println!("{r:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
