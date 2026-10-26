fn main() {
    let mut count = 0;
    let mut c = move || {
        count += 1;
        count
    };
    println!("{}", c());
    println!("{} {}", c(), count);
}
