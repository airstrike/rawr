fn main() {
    let mut v = vec![1];
    let c = v.len() == 1;
    let r = &mut v;
    if c {
        v.push(1)
    } else {
        r.push(2)
    }
    let __k = __keep(&v);
    println!("{:?}", v);
    if __c() {
        __see(__k);
    }
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
