fn make() -> String {
    String::from("made")
}

fn main() {
    let x = make().as_str();
    println!("{x}");
}
