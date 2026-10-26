fn main() {
    let a = vec![1];
    let mut b = vec![2];
    let c = a.len() > 0;
    let r = if c { &a } else { &b };
    println!("{}", r.len());
    let mut __f = || __touch(&mut b);
    let mut __g = __f;
    __g();
    __f();
    b.push(3);
    println!("{:?}", b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
