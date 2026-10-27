fn main() {
    let a = vec![1];
    let mut b = vec![2];
    let c = a.len() > 0;
    let r = if c { &a } else { &b };
    println!("{}", r.len());
    b.push(3);
    let __f = || __see(&b);
    let __g = __f;
    __f();
    println!("{:?}", b);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
