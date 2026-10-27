fn main() {
    let mut n = 1;
    let c = move || n + 1;
    n += 10;
    println!("{} {}", c(), n);
}
