fn f() -> &'static str {
    let s = "static";
    &s
}

fn main() {
    println!("{}", f());
}
