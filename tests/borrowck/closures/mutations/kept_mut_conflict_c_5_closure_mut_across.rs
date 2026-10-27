fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(1);
    v.len();
    let mut __f = || __touch(&mut c);
    c();
    __f();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
