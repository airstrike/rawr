fn main() {
    let mut o = Some(1);
    let c = || if let Some(x) = o { x } else { 0 };
    let mut __f = || __touch(&mut o);
    o = None;
    __f();
    println!("{} {:?}", c(), o);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
