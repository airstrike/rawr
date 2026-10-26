fn main() {
    let mut v = vec![1];
    let mut n = 0;
    loop {
        let r = &mut v;
        n += 1;
        if n == 2 {
            continue;
        }
        if n > 3 {
            v.push(9);
            break;
        }
        r.push(n);
    }
    let __f = || __see(&v);
    let __g = __f;
    __f();
    println!("{:?}", v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
