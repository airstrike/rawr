fn grow(v: &mut Vec<i32>) {
    let mut c = || v.push(1);
    c();
    c();
    println!("{}", v.len());
}
fn main() {
    let mut v = vec![1];
    let mut __f = || __touch(&mut v);
    __f();
    grow(&mut v);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
