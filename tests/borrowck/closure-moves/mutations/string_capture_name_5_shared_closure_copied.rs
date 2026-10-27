fn main() {
    let mut name = String::from("a");
    let mut add = |t: &str| name.push_str(t);
    add("b");
    let __f = || __see(&name);
    let __g = __f;
    __f();
    println!("{}", name.len());
    __g();
    add("c");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
