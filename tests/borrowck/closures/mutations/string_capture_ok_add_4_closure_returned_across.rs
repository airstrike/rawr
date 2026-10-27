fn main() {
    let mut name = String::from("a");
    let mut add = |t: &str| name.push_str(t);
    let __h = || &add;
    let __k = __h();
    add("b");
    __see(__k);
    add("c");
    println!("{}", name);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
