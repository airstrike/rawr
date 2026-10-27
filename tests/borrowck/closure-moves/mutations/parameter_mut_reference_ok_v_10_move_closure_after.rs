fn grow(v: &mut Vec<i32>) {
    let mut c = || v.push(1);
    c();
    c();
    println!("{}", v.len());
}
fn main() {
    let mut v = vec![1];
    grow(&mut v);
    println!("{:?}", v);
    let __m = move || __see(&v);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
