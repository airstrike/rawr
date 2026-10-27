fn takes(s: &str) -> usize {
    s.len()
}

fn main() {
    let n = takes(String::from("a").as_str());
    println!("{} {n}", String::from("b").as_str());
    match String::from("c").as_str() {
        s => println!("{s}"),
    }
    if let Some(x) = vec![1].first() {
        println!("{x}");
    }
}
