fn main() {
    let mut t = (String::from("ab"), String::from("cde"));
    let b = t.1;
    t.1 = String::from("f");
    __take(t);
    let u = t;
    println!("{} {} {}", u.0, u.1, b);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
