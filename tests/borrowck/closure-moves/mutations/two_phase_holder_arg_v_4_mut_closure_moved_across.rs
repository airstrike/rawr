fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    r.push(v.len());
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
