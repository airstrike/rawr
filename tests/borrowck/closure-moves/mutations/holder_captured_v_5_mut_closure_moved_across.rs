fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let c = || r.len();
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    v.push(1);
    __g();
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
