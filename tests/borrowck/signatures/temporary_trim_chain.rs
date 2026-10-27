fn main() {
    let words = String::from(" a b ").trim().split(' ');
    for w in words {
        println!("{w}");
    }
}
