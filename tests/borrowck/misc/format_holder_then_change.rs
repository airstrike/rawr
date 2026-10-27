fn main() {
    let mut s = String::from("hi");
    let r = &s;
    let t = format!("{} {}", r, {
        s.push('!');
        1
    });
    println!("{t}");
}
