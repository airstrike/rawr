fn main() {
    let x = 3;
    let c = || x;
    let d = c;
    println!("{} {} {}", c(), d(), x);
}
