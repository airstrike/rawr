fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let mut __f = || __touch(&mut v);
    r.push(v.len());
    __f();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
