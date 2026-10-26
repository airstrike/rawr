fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    let __t = vec![&v].to_vec();
    __see(&__t);
    v.push(3);
    let m = &mut v;
    m.push(4);
    v.push(5);
    println!("{n} {:?}", v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
