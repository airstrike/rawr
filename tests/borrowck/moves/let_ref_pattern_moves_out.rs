fn main() {
    let r = &String::from("ab");
    let &s = r;
    println!("{}", s);
}
