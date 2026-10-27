fn main() {
    let mut v = vec![1, 2];
    let mut calls = Vec::new();
    calls.push(|| v.len());
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    v.push(3);
    __g();
    println!("{}", calls[0]());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
