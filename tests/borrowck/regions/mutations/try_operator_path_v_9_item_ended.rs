fn first(v: &mut Vec<i64>) -> Option<i64> {
    let r = &mut *v;
    let x = r.first().copied()?;
    v.push(x);
    Some(x)
}
fn main() {
    let mut v = vec![4];
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    println!("{:?} {:?}", first(&mut v), v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
