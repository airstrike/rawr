fn main() {
    let mut v = vec![1];
    for i in 0..3 {
        let mut c = || v.push(i);
        let mut __f = || __touch(&mut c);
        c();
        __f();
        println!("{}", v.len());
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
