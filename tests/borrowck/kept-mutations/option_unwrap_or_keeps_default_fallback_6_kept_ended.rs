fn main() {
    let a = 1;
    let mut fallback = 0;
    let o: Option<&i32> = None;
    let r = o.unwrap_or(&fallback);
    let __k = __keep(&fallback);
    __see(__k);
    fallback += 1;
    println!("{r} {a}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
