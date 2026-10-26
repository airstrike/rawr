struct Label {
    text: &'static str,
}

fn main() {
    let label = Label { text: "x" };
    println!("{}", label.text);
}
