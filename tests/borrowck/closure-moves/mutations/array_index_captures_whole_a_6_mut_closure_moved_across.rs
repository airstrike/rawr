fn main() {
    let mut a = [1, 2];
    let mut c = || a[0] += 1;
    let x = a[1];
    c();
    let mut __f = || __touch(&mut a);
    let mut __g = __f;
    println!("{} {:?}", x, a);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
