fn main() {
    let mut v = vec![1];
    for i in 0..3 {
        let mut c = || v.push(i);
        let __f = || __see(&c);
        let __g = __f;
        __f();
        c();
        __g();
        println!("{}", v.len());
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
