fn main() {
    let mut v = vec![1, 2];
    let c = || v.len() + 1;
    let n = c();
    v.push(3);
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    __g();
    __f();
    println!("{} {:?}", n, v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
