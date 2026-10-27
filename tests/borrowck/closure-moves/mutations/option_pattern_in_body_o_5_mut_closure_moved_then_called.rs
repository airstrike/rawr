fn main() {
    let mut o = Some(1);
    let c = || if let Some(x) = o { x } else { 0 };
    o = None;
    let mut __f = || __touch(&mut o);
    let mut __g = __f;
    __g();
    __f();
    println!("{} {:?}", c(), o);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
