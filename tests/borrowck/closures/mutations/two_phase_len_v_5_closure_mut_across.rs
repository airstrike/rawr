fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let mut __f = || __touch(&mut v);
    v.push(r.len());
    __f();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
