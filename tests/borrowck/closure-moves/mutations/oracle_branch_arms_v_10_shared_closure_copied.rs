fn main() {
    let mut v = vec![1];
    let c = v.len() == 1;
    let r = &mut v;
    if c {
        v.push(1)
    } else {
        r.push(2)
    }
    let __f = || __see(&v);
    let __g = __f;
    __f();
    println!("{:?}", v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
