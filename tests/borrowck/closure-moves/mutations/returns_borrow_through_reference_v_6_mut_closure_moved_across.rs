fn main() {
    let mut v = vec![1, 2];
    let s = &v;
    let c = || &s[0];
    let r = c();
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    v.push(3);
    __g();
    println!("{}", r);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
