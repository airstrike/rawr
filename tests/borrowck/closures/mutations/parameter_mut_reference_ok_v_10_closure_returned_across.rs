fn grow(v: &mut Vec<i32>) {
    let mut c = || v.push(1);
    c();
    c();
    println!("{}", v.len());
}
fn main() {
    let mut v = vec![1];
    grow(&mut v);
    let __h = || &v;
    let __k = __h();
    println!("{:?}", v);
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
