fn main() {
    let mut n = 0;
    while {
        let mut k = 0;
        loop {
            k += 1;
            if k > 2 {
                break;
            }
        }
        n < k
    } {
        n += 1;
    }
    println!("{n}");
}
