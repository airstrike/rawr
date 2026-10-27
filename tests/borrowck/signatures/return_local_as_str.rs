fn f() -> &'static str {
    let s = String::from("a");
    s.as_str()
}

fn main() {
    println!("{}", f());
}
