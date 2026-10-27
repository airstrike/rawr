fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let c = || r.len();
    println!("{}", c());
    v.push(1);
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    println!("{:?}", v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
