fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(v.len());
    let mut __f = || __touch(&mut v);
    println!("{:?}", v);
    if __c() {
        __f();
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
