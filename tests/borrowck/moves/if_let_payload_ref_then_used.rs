fn main() {
    let s = Some(String::from("ab"));
    if let Some(ref x) = s {
        println!("{}", x);
    }
    println!("{:?}", s);
}
