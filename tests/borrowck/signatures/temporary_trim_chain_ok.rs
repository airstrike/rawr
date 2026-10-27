fn main() {
    for w in String::from(" a b ").trim().split(' ') {
        println!("{w}");
    }
    let count = String::from(" a b ").trim().split(' ').count();
    println!("{count}");
}
