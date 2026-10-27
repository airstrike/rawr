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
    match n {
        __v => __take(__v),
    }
    println!("{n}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
