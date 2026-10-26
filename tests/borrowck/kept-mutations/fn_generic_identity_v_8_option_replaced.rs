fn id<T>(x: T) -> T {
    x
}

fn main() {
    let mut v = vec![1];
    let r = id(&v);
    let mut __o = Some(&v);
    __see(&__o);
    __o = None;
    v.push(2);
    __see(&__o);
    println!("{r:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
