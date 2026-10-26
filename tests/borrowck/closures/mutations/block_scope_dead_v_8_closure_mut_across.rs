fn main() {
    let mut v = vec![1];
    {
        let r = &mut v;
        r.push(2);
    }
    v.push(3);
    let mut __f = || __touch(&mut v);
    println!("{:?}", v);
    __f();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
