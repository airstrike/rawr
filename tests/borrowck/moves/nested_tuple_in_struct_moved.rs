struct S {
    pair: (String, String),
}

fn main() {
    let s = S { pair: (String::from("ab"), String::from("cde")) };
    let a = s.pair.0;
    println!("{} {}", a, s.pair.1);
}
