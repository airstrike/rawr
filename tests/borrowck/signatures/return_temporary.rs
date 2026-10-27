fn f() -> &'static str {
    String::from("a").as_str()
}

fn main() {
    println!("{}", f());
}
