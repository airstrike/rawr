fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __m = move || __see(&s);
    s.push('b');
    __m();
    println!("{r}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
