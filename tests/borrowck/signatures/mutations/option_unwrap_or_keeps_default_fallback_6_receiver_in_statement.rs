fn main() {
    let a = 1;
    let mut fallback = 0;
    let o: Option<&i32> = None;
    let r = o.unwrap_or(&fallback);
    __see(&vec![&fallback].first());
    fallback += 1;
    println!("{r} {a}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
