fn main() {
    let a = vec![1];
    let mut b = vec![2];
    let c = || {
        b.len();
        &a
    };
    let r = c();
    let mut __f = || __touch(&mut b);
    b.push(3);
    if __c() {
        __f();
    }
    println!("{:?} {:?}", r, b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
