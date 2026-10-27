fn main() {
    let mut v = vec![1];
    let mut a = || v.push(1);
    a();
    let mut b = || v.push(2);
    let mut __f = || __touch(&mut b);
    let mut __g = __f;
    b();
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
