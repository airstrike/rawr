fn main() {
    let mut o = Some(1);
    let c = || if let Some(x) = o { x } else { 0 };
    o = None;
    let __f = || __see(&o);
    let __g = __f;
    __f();
    println!("{} {:?}", c(), o);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
