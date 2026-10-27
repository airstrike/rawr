fn grow(v: &mut Vec<i32>) {
    let mut c = || v.push(1);
    v.len();
    c();
}
fn main() {
    let mut v = vec![1];
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    grow(&mut v);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
