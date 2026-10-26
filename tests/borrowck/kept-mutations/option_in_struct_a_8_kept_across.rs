struct Slot<'a> {
    value: Option<&'a i32>,
}

fn main() {
    let mut a = 1;
    let slot = Slot { value: Some(&a) };
    let __k = __keep(&a);
    a += 1;
    __see(__k);
    println!("{:?}", slot.value);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
