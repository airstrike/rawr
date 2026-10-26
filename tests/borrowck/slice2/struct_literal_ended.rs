struct View<'a> {
    name: &'a String,
}

fn main() {
    let mut name = String::from("a");
    let view = View { name: &name };
    println!("{}", view.name);
    name = String::from("b");
    println!("{name}");
}
