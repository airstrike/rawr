fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __g = || __see(&s);
    s.push('b');
    __g();
    println!("{r}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
