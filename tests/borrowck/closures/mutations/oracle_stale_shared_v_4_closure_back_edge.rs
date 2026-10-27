fn main() {
    let mut v = vec![1];
    let r = &v;
    let mut __f = || __touch(&mut v);
    for __i in 0..2 {
        __f();
        v.push(1);
    }
    println!("{r:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
