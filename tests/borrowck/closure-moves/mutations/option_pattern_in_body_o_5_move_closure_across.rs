fn main() {
    let mut o = Some(1);
    let c = || if let Some(x) = o { x } else { 0 };
    o = None;
    let __m = move || __see(&o);
    println!("{} {:?}", c(), o);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
