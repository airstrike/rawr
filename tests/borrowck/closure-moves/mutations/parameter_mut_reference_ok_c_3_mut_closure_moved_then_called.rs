fn grow(v: &mut Vec<i32>) {
    let mut c = || v.push(1);
    let mut __f = || __touch(&mut c);
    let mut __g = __f;
    __g();
    __f();
    c();
    c();
    println!("{}", v.len());
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
