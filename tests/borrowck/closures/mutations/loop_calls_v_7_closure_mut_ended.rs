fn main() {
    let mut v = vec![1];
    let mut c = || v.push(1);
    for _ in 0..3 {
        c();
    }
    let mut __f = || __touch(&mut v);
    __f();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
