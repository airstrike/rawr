fn main() {
    let mut v = vec![1, 2];
    let c = || v.len() + 1;
    let n = c();
    let mut __f = || __touch(&mut v);
    v.push(3);
    __f();
    println!("{} {:?}", n, v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
