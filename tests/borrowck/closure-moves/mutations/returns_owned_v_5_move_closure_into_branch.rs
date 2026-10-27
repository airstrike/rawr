fn main() {
    let mut v = vec![1, 2];
    let c = || v.len() + 1;
    let n = c();
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    v.push(3);
    println!("{} {:?}", n, v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
