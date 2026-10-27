fn main() {
    let mut v = vec![1, 2];
    let mut __f = || __touch(&mut v);
    v.push(v.len());
    if __c() {
        __f();
    }
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
