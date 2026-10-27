struct D<'a>(&'a String);

impl Drop for D<'_> {
    fn drop(&mut self) {}
}

fn main() {
    let mut t = String::from("b");
    let d = D(&t);
    drop(d);
    t.push('x');
    println!("{}", t);
}
