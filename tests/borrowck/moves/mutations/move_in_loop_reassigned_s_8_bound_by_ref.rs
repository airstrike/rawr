fn main() {
    let mut s = String::from("ab");
    for i in 0..3 {
        let t = s;
        println!("{}", t);
        s = format!("{}", i);
    }
    let ref __v = s;
    println!("{}", s);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
