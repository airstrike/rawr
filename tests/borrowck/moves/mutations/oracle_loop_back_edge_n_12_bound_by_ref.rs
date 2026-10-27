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
    let ref __v = n;
    println!("{n}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
