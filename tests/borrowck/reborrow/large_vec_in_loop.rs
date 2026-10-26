fn main() {
    let mut low = Vec::new();
    let mut high = Vec::new();
    let mut r = &mut low;
    for n in 0..20000 {
        r.push(n);
        if n == 9999 {
            r = &mut high;
        }
    }
    let low_sum: i64 = low.iter().sum();
    let high_sum: i64 = high.iter().sum();
    println!("{} {} {} {}", low.len(), high.len(), low_sum, high_sum);
}
