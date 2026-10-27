fn main() {
    let b = Box::new(String::from("ab"));
    let s = *b;
    println!("{}", s);
}
