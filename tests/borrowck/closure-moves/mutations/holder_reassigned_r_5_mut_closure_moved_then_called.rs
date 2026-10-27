fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    let mut __f = || __touch(&mut r);
    let mut __g = __f;
    __g();
    __f();
    println!("{}", r.len());
    r = &b;
    a.push(3);
    println!("{} {:?}", r.len(), a);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
