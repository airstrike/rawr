use std::collections::HashMap;

struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Parser { text, at: 0 }
    }

    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn text(&self) -> &str {
        self.text
    }
}

fn first<'a>(v: &'a [String]) -> &'a str {
    &v[0]
}

fn lookup<'a>(m: &'a HashMap<String, String>, k: &str) -> Option<&'a String> {
    m.get(k)
}

fn longest_with<'a, T: std::fmt::Display>(x: &'a str, y: &'a str, note: T) -> &'a str {
    println!("{note}");
    if x.len() > y.len() { x } else { y }
}

fn split_first(s: &str) -> (&str, &str) {
    match s.split_once(' ') {
        Some(pair) => pair,
        None => (s, ""),
    }
}

fn main() {
    let p = Parser::new("hello world");
    let words = vec![String::from("a"), String::from("b")];
    let mut m = HashMap::new();
    m.insert(String::from("k"), String::from("v"));
    println!("{} {} {} {:?}", p.rest(), p.text(), first(&words), lookup(&m, "k"));
    println!("{} {:?}", longest_with("ab", "c", 1), split_first("x y"));
}
