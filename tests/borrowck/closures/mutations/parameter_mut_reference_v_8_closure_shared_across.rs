fn grow(v: &mut Vec<i32>) {
    let mut c = || v.push(1);
    v.len();
    c();
}
fn main() {
    let mut v = vec![1];
    let __g = || __see(&v);
    grow(&mut v);
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
