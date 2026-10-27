fn main() {
    let mut x = 5;
    let r = &mut x;
    let y = x;
    *r += 1;
    let mut __f = || __touch(&mut x);
    __f();
    println!("{} {}", x, y);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
