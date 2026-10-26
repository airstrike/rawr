fn main() {
    let a = 1;
    let mut fallback = 0;
    let o: Option<&i32> = None;
    let r = o.unwrap_or(&fallback);
    let mut __o = Some(&fallback);
    __see(&__o);
    __o = None;
    fallback += 1;
    __see(&__o);
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
