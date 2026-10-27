fn f(o: &Option<String>) -> usize {
    o.map(|s| s.len()).unwrap_or(0)
}

fn main() {
    println!("{}", f(&Some(String::from("ab"))));
}
