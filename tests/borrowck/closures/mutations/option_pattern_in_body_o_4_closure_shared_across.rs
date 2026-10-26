fn main() {
    let mut o = Some(1);
    let c = || if let Some(x) = o { x } else { 0 };
    let __g = || __see(&o);
    o = None;
    __g();
    println!("{} {:?}", c(), o);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
