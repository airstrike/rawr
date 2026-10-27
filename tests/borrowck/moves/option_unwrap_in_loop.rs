fn main() {
    let o = Some(String::from("ab"));
    for _ in 0..2 {
        println!("{}", o.unwrap());
    }
}
