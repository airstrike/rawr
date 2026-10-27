fn main() {
    let s = String::from("ab");
    let c = s.len() > 1;
    let t = match c {
        true => s,
        false => String::new(),
    };
    println!("{} {}", t, s);
}
