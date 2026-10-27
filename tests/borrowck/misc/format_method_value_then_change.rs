fn main() {
    let mut s = String::from("hi");
    let t = format!("{} {}", s.as_str(), {
        s.push('!');
        1
    });
    println!("{t}");
}
