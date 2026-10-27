fn first(v: &mut Vec<i64>) -> Option<i64> {
    let r = &mut *v;
    let x = r.first().copied()?;
    v.push(x);
    Some(x)
}
fn main() {
    let mut v = vec![4];
    println!("{:?} {:?}", first(&mut v), v);
    let __m = move || __see(&v);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
