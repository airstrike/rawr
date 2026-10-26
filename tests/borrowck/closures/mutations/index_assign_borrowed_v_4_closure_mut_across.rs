fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let mut __f = || __touch(&mut v);
    v[0] = 5;
    __f();
    println!("{}", r[0]);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
