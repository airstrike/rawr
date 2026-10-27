fn main() {
    let mut n = 1;
    let c = || n + 1;
    n = 5;
    println!("{}", c());
}
