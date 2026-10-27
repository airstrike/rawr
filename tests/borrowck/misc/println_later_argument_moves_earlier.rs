fn shout(x: String) -> String {
    x.to_uppercase()
}

fn main() {
    let p = String::from("hi");
    println!("{} {}", p, shout(p));
}
