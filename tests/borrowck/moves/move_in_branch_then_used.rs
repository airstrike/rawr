fn main() {
    let s = String::from("ab");
    if s.len() > 1 {
        let t = s;
        println!("{}", t);
    }
    println!("{}", s);
}
