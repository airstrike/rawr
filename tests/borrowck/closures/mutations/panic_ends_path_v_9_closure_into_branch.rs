fn main() {
    let mut v = vec![1];
    let r = &mut v;
    if v.is_empty() {
        v.push(1);
        panic!("empty");
    }
    r.push(2);
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
