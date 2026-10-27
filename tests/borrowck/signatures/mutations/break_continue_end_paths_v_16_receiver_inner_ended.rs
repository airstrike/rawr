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
    let __t = vec![&v].to_vec();
    __see(&__t);
    println!("{:?}", v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
