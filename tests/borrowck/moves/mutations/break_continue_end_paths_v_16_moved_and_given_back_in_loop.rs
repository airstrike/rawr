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
    for __i in 0..2 {
        let __m = v;
        v = __m;
    }
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
