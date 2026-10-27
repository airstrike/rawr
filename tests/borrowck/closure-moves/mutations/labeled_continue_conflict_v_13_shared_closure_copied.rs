fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut n = 0;
    'outer: while n < 3 {
        n += 1;
        loop {
            v.push(n);
            continue 'outer;
        }
    }
    r.push(0);
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
