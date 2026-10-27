fn main() {
    let s = Some(String::from("ab"));
    if let Some(x) = &s {
        println!("{}", x);
    }
    println!("{:?}", s);
}
