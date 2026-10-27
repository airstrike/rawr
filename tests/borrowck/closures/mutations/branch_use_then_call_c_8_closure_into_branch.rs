fn main() {
    let mut v = vec![1];
    let flag = v.len() > 0;
    let mut c = || v.push(2);
    if flag {
        println!("{}", v.len());
    }
    let mut __f = || __touch(&mut c);
    c();
    if __c() {
        __f();
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
