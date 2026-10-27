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
    let __t = &__id(&source);
    __see(__t);
    source.push_str("d");
    println!("{t}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
