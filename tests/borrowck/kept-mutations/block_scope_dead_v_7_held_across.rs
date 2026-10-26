fn main() {
    let mut v = vec![1];
    {
        let r = &mut v;
        r.push(2);
    }
    let __h = __Hold(&v);
    v.push(3);
    __see(&__h);
    println!("{:?}", v);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
