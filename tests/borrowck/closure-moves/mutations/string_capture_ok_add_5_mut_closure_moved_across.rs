fn main() {
    let mut name = String::from("a");
    let mut add = |t: &str| name.push_str(t);
    add("b");
    let mut __f = || __touch(&mut add);
    let mut __g = __f;
    add("c");
    __g();
    println!("{}", name);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
