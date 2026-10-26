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
    let __h = __Hold(&source);
    source.push_str("d");
    __see(&__h);
    println!("{t}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
