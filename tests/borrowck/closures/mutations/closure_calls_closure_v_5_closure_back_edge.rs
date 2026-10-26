fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    let d = || c() + 1;
    let mut __f = || __touch(&mut v);
    for __i in 0..2 {
        __f();
        v.push(1);
    }
    println!("{}", d());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
