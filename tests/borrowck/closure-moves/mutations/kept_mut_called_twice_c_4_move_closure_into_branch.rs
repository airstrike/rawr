fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(3);
    if __c() {
        let __m = move || __see(&c);
        __m();
    }
    c();
    c();
    println!("{}", v.len());
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
