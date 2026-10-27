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
    println!("{:?}", v);
    __take(v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
