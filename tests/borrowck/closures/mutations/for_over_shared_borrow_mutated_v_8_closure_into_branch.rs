fn main() {
    let mut v = vec![1, 2];
    for x in &v {
        if *x > 5 {
            v.push(1);
        }
    }
    let mut __f = || __touch(&mut v);
    println!("{:?}", v);
    if __c() {
        __f();
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
