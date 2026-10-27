fn main() {
    let mut v = vec![1, 2];
    let mut calls = Vec::new();
    calls.push(|| v.len());
    v.push(3);
    let __f = || __see(&calls);
    let __g = __f;
    __f();
    println!("{}", calls[0]());
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
