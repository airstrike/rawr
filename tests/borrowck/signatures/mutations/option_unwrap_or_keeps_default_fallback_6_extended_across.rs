fn main() {
    let a = 1;
    let mut fallback = 0;
    let o: Option<&i32> = None;
    let r = o.unwrap_or(&fallback);
    let __t = &__id(&fallback);
    fallback += 1;
    __see(__t);
    println!("{r} {a}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
