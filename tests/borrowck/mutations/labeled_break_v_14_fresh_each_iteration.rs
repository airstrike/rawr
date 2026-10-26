fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut n = 0;
    'outer: loop {
        loop {
            n += 1;
            if n > 2 {
                break 'outer;
            }
            r.push(n);
        }
    }
    for __i in 0..2 {
        let __b = &mut v;
        __touch(__b);
        v.push(7);
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
