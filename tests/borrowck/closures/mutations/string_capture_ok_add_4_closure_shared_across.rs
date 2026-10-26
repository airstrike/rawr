fn main() {
    let mut name = String::from("a");
    let mut add = |t: &str| name.push_str(t);
    let __g = || __see(&add);
    add("b");
    __g();
    add("c");
    println!("{}", name);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
