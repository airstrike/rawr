fn main() {
    let mut v = vec![1];
    let mut a = || v.push(1);
    let mut __f = || __touch(&mut a);
    let mut __g = __f;
    a();
    __g();
    let mut b = || v.push(2);
    b();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
