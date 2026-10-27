fn bump(r: &mut i32) {
    let c = || *r += 1;
    let mut d = c;
    d();
    d();
}

fn main() {
    let mut n = 0;
    bump(&mut n);
    let mut __f = || __touch(&mut n);
    let mut __g = __f;
    println!("{}", n);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
