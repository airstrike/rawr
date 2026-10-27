struct D<'a>(&'a String);

impl Drop for D<'_> {
    fn drop(&mut self) {}
}

fn main() {
    let s = String::from("a");
    let mut t = String::from("b");
    let mut d = D(&s);
    drop(d);
    d = D(&t);
    t.push('x');
    println!("{}", t);
}
