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
    let __v = vec![&reader];
    reader = Reader { text: "other" };
    __see(&__v);
    println!("{t} {}", reader.text);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
