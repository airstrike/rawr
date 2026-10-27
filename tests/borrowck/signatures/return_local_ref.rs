fn f() -> &'static str {
    let s = String::new();
    &s
}

fn main() {
    println!("{}", f());
}
