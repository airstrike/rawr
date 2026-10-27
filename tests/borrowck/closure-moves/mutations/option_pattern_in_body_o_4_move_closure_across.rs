fn main() {
    let mut o = Some(1);
    let c = || if let Some(x) = o { x } else { 0 };
    let __m = move || __see(&o);
    o = None;
    __m();
    println!("{} {:?}", c(), o);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
