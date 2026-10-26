fn main() {
    let mut total = 0;
    let r = &mut total;
    let mut total = 100;
    for i in 1..4 {
        *r += i;
        total += 1;
    }
    println!("{total} {r}");
}
