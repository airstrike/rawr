fn main() {
    let mut val = Some(String::from("ab"));
    while let Some(s) = val {
        println!("{}", s);
        if s.len() > 1 {
            val = None;
        }
    }
}
