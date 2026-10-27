fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    v.push(3);
    let m = &mut v;
    m.push(4);
    let __t = &__id(&v);
    v.push(5);
    __see(__t);
    println!("{n} {:?}", v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
