fn main() {
    let s = String::from("ab");
    if s.len() > 5 {
        let t = s;
        println!("{}", t);
        return;
    }
    println!("{}", s);
}
