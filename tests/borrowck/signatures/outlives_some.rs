fn wrap<'a, 'b>(_x: &'a str, y: &'b str) -> Option<&'a str> {
    Some(y)
}

fn main() {
    println!("{:?}", wrap("a", "b"));
}
