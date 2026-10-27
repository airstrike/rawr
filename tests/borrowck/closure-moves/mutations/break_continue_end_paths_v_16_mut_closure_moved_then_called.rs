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
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    __g();
    __f();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
