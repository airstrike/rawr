fn grow(v: &mut Vec<i32>) {
    let mut c = || v.push(1);
    v.len();
    let mut __f = || __touch(&mut c);
    c();
    __f();
}
fn main() {
    let mut v = vec![1];
    grow(&mut v);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
