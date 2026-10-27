fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    println!("{}", c());
    v.push(3);
    let mut __f = || __touch(&mut v);
    for __i in 0..2 {
        __f();
        println!("{:?}", v);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
