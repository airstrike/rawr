fn main() {
    let mut s = String::from("a");
    let r = &s;
    let mut __f = || __touch(&mut s);
    let mut __g = __f;
    s.push('b');
    __g();
    println!("{r}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
