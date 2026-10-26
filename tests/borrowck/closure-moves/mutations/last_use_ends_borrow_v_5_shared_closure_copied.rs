fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    let __f = || __see(&v);
    let __g = __f;
    __f();
    v.push(3);
    __g();
    let m = &mut v;
    m.push(4);
    v.push(5);
    println!("{n} {:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
