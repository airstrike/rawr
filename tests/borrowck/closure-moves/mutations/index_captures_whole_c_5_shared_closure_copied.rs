fn main() {
    let mut v = vec![1, 2];
    let mut c = || v[0] += 1;
    println!("{}", v[1]);
    let __f = || __see(&c);
    let __g = __f;
    __f();
    c();
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
