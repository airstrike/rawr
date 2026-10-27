fn main() {
    let mut a = 1;
    let o = Some(&a);
    let r = o.unwrap();
    let __t = &__id(&a);
    __see(__t);
    a += 1;
    println!("{r}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
