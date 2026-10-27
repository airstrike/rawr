fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut n = 0;
    loop {
        r.push(1);
        n += v.len();
        if n > 3 {
            break;
        }
    }
    let __f = || __see(&n);
    let __g = __f;
    __f();
    println!("{n}");
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
