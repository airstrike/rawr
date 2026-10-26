fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __h = || &s;
    let __k = __h();
    s.push('b');
    __see(__k);
    println!("{r}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
