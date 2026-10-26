fn main() {
    let mut a = 1;
    let o = Some(&a);
    let r = o.unwrap();
    let __k = __keep(&a);
    __see(__k);
    a += 1;
    println!("{r}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
