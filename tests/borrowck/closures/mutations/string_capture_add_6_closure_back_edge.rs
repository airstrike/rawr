fn main() {
    let mut name = String::from("a");
    let mut add = |t: &str| name.push_str(t);
    add("b");
    println!("{}", name.len());
    let mut __f = || __touch(&mut add);
    for __i in 0..2 {
        __f();
        add("c");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
