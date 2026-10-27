fn first(v: &mut Vec<i64>) -> Option<i64> {
    let r = &mut *v;
    let x = r.first().copied()?;
    v.push(x);
    Some(x)
}
fn main() {
    let mut v = vec![4];
    __see(&vec![&v].first());
    println!("{:?} {:?}", first(&mut v), v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
