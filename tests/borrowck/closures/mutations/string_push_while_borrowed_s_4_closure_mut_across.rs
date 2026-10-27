fn main() {
    let mut s = String::from("a");
    let r = &s;
    let mut __f = || __touch(&mut s);
    s.push('b');
    __f();
    println!("{r}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
