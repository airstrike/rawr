fn main() {
    let mut v = vec![1];
    let flag = v.len() > 0;
    let mut c = || v.push(2);
    if flag {
        println!("{}", v.len());
    }
    let mut __f = || __touch(&mut c);
    for __i in 0..2 {
        __f();
        c();
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
