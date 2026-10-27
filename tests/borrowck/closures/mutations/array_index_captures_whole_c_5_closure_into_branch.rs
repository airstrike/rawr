fn main() {
    let mut a = [1, 2];
    let mut c = || a[0] += 1;
    let x = a[1];
    let mut __f = || __touch(&mut c);
    c();
    if __c() {
        __f();
    }
    println!("{} {:?}", x, a);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
