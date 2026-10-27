fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    println!("{}", r.len());
    r = &b;
    a.push(3);
    println!("{} {:?}", r.len(), a);
    __take(a);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
