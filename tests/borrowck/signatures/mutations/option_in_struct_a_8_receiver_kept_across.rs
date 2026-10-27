struct Slot<'a> {
    value: Option<&'a i32>,
}

fn main() {
    let mut a = 1;
    let slot = Slot { value: Some(&a) };
    let __t = vec![&a].first();
    a += 1;
    __see(&__t);
    println!("{:?}", slot.value);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
