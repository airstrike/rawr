fn main() {
    let mut x = 5;
    let r = &mut x;
    let y = x;
    *r += 1;
    let mut __f = || __touch(&mut x);
    let mut __g = __f;
    __g();
    __f();
    println!("{} {}", x, y);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
