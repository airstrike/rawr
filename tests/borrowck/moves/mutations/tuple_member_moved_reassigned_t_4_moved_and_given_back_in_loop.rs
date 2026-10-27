fn main() {
    let mut t = (String::from("ab"), String::from("cde"));
    let b = t.1;
    for __i in 0..2 {
        let __m = t;
        t = __m;
    }
    t.1 = String::from("f");
    let u = t;
    println!("{} {} {}", u.0, u.1, b);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
