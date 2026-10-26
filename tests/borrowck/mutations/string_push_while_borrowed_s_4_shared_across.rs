fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __s = &s;
    s.push('b');
    __see(__s);
    println!("{r}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
