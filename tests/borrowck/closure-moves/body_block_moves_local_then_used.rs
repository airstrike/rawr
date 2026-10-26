fn main() {
    let x = String::from("ab");
    let c = || {
        let n = 1;
        x
    };
    let y = x;
    println!("{} {}", c(), y);
}
