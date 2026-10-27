struct Pair {
    a: String,
    b: String,
}

fn shout(x: String) -> String {
    x.to_uppercase()
}

fn main() {
    let pair = Pair { a: String::from("a"), b: String::from("b") };
    let s = format!("{} {}", pair.a, shout(pair.b));
    println!("{s} {}", pair.a);
}
