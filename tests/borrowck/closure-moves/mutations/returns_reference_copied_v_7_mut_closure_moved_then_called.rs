fn main() {
    let mut v = vec![1, 2];
    let w = vec![3];
    let s = &w;
    let c = || s;
    let r = c();
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    __g();
    __f();
    v.push(3);
    println!("{:?} {:?}", r, v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
