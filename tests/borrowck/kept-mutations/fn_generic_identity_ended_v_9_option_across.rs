fn id<T>(x: T) -> T {
    x
}

fn main() {
    let mut v = vec![1];
    let r = id(&v);
    println!("{r:?}");
    let __o = Some(&v);
    v.push(2);
    __see(&__o);
    println!("{v:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
