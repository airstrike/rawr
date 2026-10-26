fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    println!("{}", r.len());
    r = &b;
    a.push(3);
    let mut __f = || __touch(&mut a);
    println!("{} {:?}", r.len(), a);
    __f();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
