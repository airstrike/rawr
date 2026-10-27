fn main() {
    let mut a = 1;
    let o = Some(&a);
    let r = o.unwrap();
    __see(&vec![&a].first());
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
