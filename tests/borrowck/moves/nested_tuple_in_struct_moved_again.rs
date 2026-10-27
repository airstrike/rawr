struct S {
    pair: (String, String),
}

fn main() {
    let s = S { pair: (String::from("ab"), String::from("cde")) };
    let a = s.pair.0;
    let p = s.pair;
    println!("{} {}", a, p.1);
}
