fn first(v: &Vec<i32>) -> &i32 {
    &v[0]
}

fn main() {
    let mut v = vec![1, 2, 3];
    let f = first(&v);
    println!("{f}");
    let __o = Some(&v);
    v.push(4);
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
