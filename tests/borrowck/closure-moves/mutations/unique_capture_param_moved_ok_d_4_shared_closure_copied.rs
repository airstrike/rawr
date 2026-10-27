fn bump(r: &mut i32) {
    let c = || *r += 1;
    let mut d = c;
    let __f = || __see(&d);
    let __g = __f;
    __f();
    d();
    __g();
    d();
}

fn main() {
    let mut n = 0;
    bump(&mut n);
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
