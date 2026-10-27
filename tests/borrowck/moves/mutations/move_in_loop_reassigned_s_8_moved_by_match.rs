fn main() {
    let mut s = String::from("ab");
    for i in 0..3 {
        let t = s;
        println!("{}", t);
        s = format!("{}", i);
    }
    match s {
        __v => __take(__v),
    }
    println!("{}", s);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
