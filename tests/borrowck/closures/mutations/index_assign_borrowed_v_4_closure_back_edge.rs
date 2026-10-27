fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let mut __f = || __touch(&mut v);
    for __i in 0..2 {
        __f();
        v[0] = 5;
    }
    println!("{}", r[0]);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
