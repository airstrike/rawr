fn main() {
    let mut name = String::from("a");
    let mut add = |t: &str| name.push_str(t);
    add("b");
    println!("{}", name.len());
    let mut __f = || __touch(&mut add);
    add("c");
    __f();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
