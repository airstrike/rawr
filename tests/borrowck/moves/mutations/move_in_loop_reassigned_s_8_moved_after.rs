fn main() {
    let mut s = String::from("ab");
    for i in 0..3 {
        let t = s;
        println!("{}", t);
        s = format!("{}", i);
    }
    println!("{}", s);
    __take(s);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
