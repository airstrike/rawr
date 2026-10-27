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
    let __t = vec![&reader].first();
    reader = Reader { text: "other" };
    __see(&__t);
    println!("{t} {}", reader.text);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
