fn main() {
    let s = String::from("ab");
    let t = {
        let n = 1;
        s
    };
    println!("{} {}", t, s);
}
