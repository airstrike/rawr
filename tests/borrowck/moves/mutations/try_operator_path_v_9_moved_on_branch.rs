fn first(v: &mut Vec<i64>) -> Option<i64> {
    let r = &mut *v;
    let x = r.first().copied()?;
    v.push(x);
    Some(x)
}
fn main() {
    let mut v = vec![4];
    if __c() {
        __take(v);
    }
    println!("{:?} {:?}", first(&mut v), v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
