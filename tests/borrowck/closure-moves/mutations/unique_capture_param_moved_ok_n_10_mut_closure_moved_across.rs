fn bump(r: &mut i32) {
    let c = || *r += 1;
    let mut d = c;
    d();
    d();
}

fn main() {
    let mut n = 0;
    let mut __f = || __touch(&mut n);
    let mut __g = __f;
    bump(&mut n);
    __g();
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
