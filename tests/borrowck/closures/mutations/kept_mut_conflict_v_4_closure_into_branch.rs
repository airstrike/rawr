fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(1);
    let mut __f = || __touch(&mut v);
    v.len();
    if __c() {
        __f();
    }
    c();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
