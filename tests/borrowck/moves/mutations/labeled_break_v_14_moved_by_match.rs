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
    match v {
        __v => __take(__v),
    }
    v.push(7);
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
