fn main() {
    let a = vec![1];
    let mut b = vec![2];
    let c = a.len() > 0;
    let r = if c { &a } else { &b };
    println!("{}", r.len());
    b.push(3);
    let __m = { b };
    println!("{:?}", b);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
