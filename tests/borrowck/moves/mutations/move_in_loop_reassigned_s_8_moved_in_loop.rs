fn main() {
    let mut s = String::from("ab");
    for i in 0..3 {
        let t = s;
        println!("{}", t);
        s = format!("{}", i);
    }
    for __i in 0..2 {
        if __c() {
            __take(s);
        }
    }
    println!("{}", s);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
