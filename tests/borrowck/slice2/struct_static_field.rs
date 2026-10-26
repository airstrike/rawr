struct Label {
    text: &'static str,
}

fn main() {
    let owned = String::from("x");
    let label = Label { text: &owned };
    println!("{}", label.text);
}
