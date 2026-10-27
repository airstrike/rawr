fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let mut __f = || __touch(&mut v);
    __f();
    v.push(r.len());
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
