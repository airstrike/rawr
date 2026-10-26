fn main() {
    let mut s = String::from("  hi  ");
    let t = s.trim();
    println!("[{t}]");
    s.push_str("!");
    println!("[{s}]");
}
