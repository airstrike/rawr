struct Reader<'a> {
    text: &'a str,
}

impl<'a> Reader<'a> {
    fn text(&self) -> &'a str {
        self.text
    }
}

fn main() {
    let source = String::from("abc");
    let mut reader = Reader { text: &source };
    let t = reader.text();
    reader = Reader { text: "other" };
    println!("{t} {}", reader.text);
}
