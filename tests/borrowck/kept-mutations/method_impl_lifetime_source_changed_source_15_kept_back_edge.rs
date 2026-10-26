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
    let __k = __keep(&source);
    for __i in 0..2 {
        __see(__k);
        source.push_str("d");
    }
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
