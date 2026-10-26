fn main() {
    let mut name = String::from("a");
    let mut add = |t: &str| name.push_str(t);
    add("b");
    add("c");
    let mut __f = || __touch(&mut name);
    for __i in 0..2 {
        __f();
        println!("{}", name);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
