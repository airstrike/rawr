fn main() {
    let mut n = 1;
    let t = format!("{} {}", n, {
        n += 1;
        n
    });
    println!("{t}");
}
