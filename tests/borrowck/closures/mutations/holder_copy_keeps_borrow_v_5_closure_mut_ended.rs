fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let mut __f = || __touch(&mut v);
    __f();
    v.push(2);
    println!("{}", s.len());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
