fn main() {
    let mut name = String::from("a");
    let mut add = |t: &str| name.push_str(t);
    add("b");
    let __m = move || __see(&name);
    println!("{}", name.len());
    __m();
    add("c");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
