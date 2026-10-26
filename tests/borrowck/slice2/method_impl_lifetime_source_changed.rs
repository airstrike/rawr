struct Reader<'a> {
    text: &'a str,
}

impl<'a> Reader<'a> {
    fn text(&self) -> &'a str {
        self.text
    }
}

fn main() {
    let mut source = String::from("abc");
    let reader = Reader { text: &source };
    let t = reader.text();
    source.push_str("d");
    println!("{t}");
}
