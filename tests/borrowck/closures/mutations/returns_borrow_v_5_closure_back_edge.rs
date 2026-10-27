fn main() {
    let mut v = vec![1, 2];
    let c = || &v;
    let r = c();
    let mut __f = || __touch(&mut v);
    for __i in 0..2 {
        __f();
        v.push(3);
    }
    println!("{:?}", r);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
