fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    __take(r);
    println!("{}", r.len());
    r = &b;
    a.push(3);
    println!("{} {:?}", r.len(), a);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
