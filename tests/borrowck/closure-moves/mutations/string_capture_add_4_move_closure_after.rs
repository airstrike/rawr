fn main() {
    let mut name = String::from("a");
    let mut add = |t: &str| name.push_str(t);
    add("b");
    let __m = move || __see(&add);
    __m();
    println!("{}", name.len());
    add("c");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
