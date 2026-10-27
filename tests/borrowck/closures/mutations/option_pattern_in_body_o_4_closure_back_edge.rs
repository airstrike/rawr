fn main() {
    let mut o = Some(1);
    let c = || if let Some(x) = o { x } else { 0 };
    let mut __f = || __touch(&mut o);
    for __i in 0..2 {
        __f();
        o = None;
    }
    println!("{} {:?}", c(), o);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
