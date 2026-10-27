fn main() {
    let mut a = [1, 2];
    let mut c = || a[0] += 1;
    let x = a[1];
    let __f = || __see(&c);
    let __g = __f;
    __f();
    c();
    __g();
    println!("{} {:?}", x, a);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
