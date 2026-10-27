fn main() {
    let mut n = 1;
    let mut m = 10;
    let t = format!(
        "{} {} {}",
        n,
        {
            m += n;
            m
        },
        m
    );
    n += 1;
    println!("{t} {n}");
}
