fn main() {
    let mut v = vec![1];
    let mut a = || v.push(1);
    a();
    let mut b = || v.push(2);
    b();
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    println!("{:?}", v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
