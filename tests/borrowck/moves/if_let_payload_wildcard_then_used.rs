fn main() {
    let s = Some(String::from("ab"));
    if let Some(_) = s {
        println!("some");
    }
    println!("{:?}", s);
}
