fn main() {
    let mut n = 1;
    let mut c = move || {
        n += 1;
        n
    };
    let mut d = c;
    println!("{} {} {} {}", d(), d(), c(), n);
}
