fn main() {
    let mut t = (String::from("ab"), String::from("cde"));
    let b = t.1;
    match t {
        __v => __take(__v),
    }
    t.1 = String::from("f");
    let u = t;
    println!("{} {} {}", u.0, u.1, b);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
