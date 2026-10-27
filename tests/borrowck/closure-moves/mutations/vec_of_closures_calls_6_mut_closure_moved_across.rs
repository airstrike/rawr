fn main() {
    let mut v = vec![1, 2];
    let mut calls = Vec::new();
    calls.push(|| v.len());
    v.push(3);
    let mut __f = || __touch(&mut calls);
    let mut __g = __f;
    println!("{}", calls[0]());
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
