fn main() {
    let mut o = Some(1);
    let c = || if let Some(x) = o { x } else { 0 };
    o = None;
    if __c() {
        let __m = move || __see(&o);
        __m();
    }
    println!("{} {:?}", c(), o);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
