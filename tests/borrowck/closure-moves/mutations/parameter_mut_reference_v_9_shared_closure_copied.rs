fn grow(v: &mut Vec<i32>) {
    let mut c = || v.push(1);
    v.len();
    c();
}
fn main() {
    let mut v = vec![1];
    grow(&mut v);
    let __f = || __see(&v);
    let __g = __f;
    __f();
    println!("{:?}", v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
